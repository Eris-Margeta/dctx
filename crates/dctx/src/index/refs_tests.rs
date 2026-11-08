// SPDX-License-Identifier: Apache-2.0
//! Tests for call and import linking over lexed fixtures.
//! Runs the lexers through `prepare_structural` and the graph assembly the walk uses, then [`link`].
//! Each fixture mirrors one language of the DCX-12 card check.

use super::extract::{index_prepared, prepare_structural};
use super::refs::{FileRefs, link};
use super::{IndexedEdge, IndexedNode, TextBlob, write_endpoint};

/// Edges of `files` after linking, as `edge src>dst confidence` lines.
fn linked(files: &[(&str, &str)]) -> Vec<String> {
    let mut text = TextBlob::default();
    let mut nodes: Vec<IndexedNode> = Vec::new();
    let mut edges: Vec<IndexedEdge> = Vec::new();
    let mut spans = Vec::new();
    for (path, source) in files {
        let first = u32::try_from(nodes.len()).expect("small");
        let path_span = text.push(path);
        nodes.push(IndexedNode {
            letter: b'm',
            ordinal: 0,
            kind: "module".to_string().into(),
            name: String::new().into(),
            path: path_span,
            lines: [1, 1],
            span: [0, 0],
            summary: String::new().into(),
            summary_source: String::new().into(),
            signature: String::new().into(),
        });
        let (prepared, raw) = prepare_structural(path, source.as_bytes());
        index_prepared(
            path,
            source.as_bytes(),
            path_span,
            first,
            prepared,
            &mut nodes,
            &mut edges,
            &mut text,
        );
        spans.push(FileRefs {
            first,
            end: u32::try_from(nodes.len()).expect("small"),
            refs: raw,
        });
    }
    link(&nodes, &mut edges, &mut text, &spans);
    edges
        .iter()
        .map(|edge| {
            let (mut src, mut dst) = (String::new(), String::new());
            write_endpoint(&mut src, &nodes, &text, edge.src);
            write_endpoint(&mut dst, &nodes, &text, edge.dst);
            format!(
                "{} {src}>{dst} {}",
                text.get(edge.edge),
                text.get(edge.confidence)
            )
        })
        .collect()
}

#[test]
fn python_calls_resolve_in_file_and_through_an_import() {
    let edges = linked(&[
        (
            "a.py",
            "def alpha():\n    return beta()\n\ndef beta():\n    return 1\n",
        ),
        (
            "b.py",
            "from a import beta\n\ndef iota():\n    return beta()\n",
        ),
    ]);
    assert!(
        edges.contains(&"calls s:a.py#alpha>s:a.py#beta name".to_string()),
        "{edges:?}"
    );
    assert!(
        edges.contains(&"calls s:b.py#iota>s:a.py#beta name".to_string()),
        "{edges:?}"
    );
    assert!(
        edges.contains(&"imports m:b.py>s:a.py#beta exact".to_string()),
        "{edges:?}"
    );
}

#[test]
fn other_languages_link_plain_calls() {
    let edges = linked(&[
        (
            "m.ts",
            "export function gamma(): number { return delta(); }\nfunction delta(): number { return 2; }\n",
        ),
        (
            "g.go",
            "package main\n\nfunc Eps() int { return zeta() }\n\nfunc zeta() int { return 3 }\n",
        ),
        (
            "J.java",
            "class J {\n  int kappa() { return lam(); }\n  int lam() { return 5; }\n}\n",
        ),
    ]);
    for want in [
        "s:m.ts#gamma>s:m.ts#delta",
        "s:g.go#Eps>s:g.go#zeta",
        "s:J.java#J::kappa>s:J.java#J::lam",
    ] {
        assert!(
            edges
                .iter()
                .any(|edge| edge.starts_with("calls ") && edge.contains(want)),
            "{want} in {edges:?}"
        );
    }
}

#[test]
fn an_ambiguous_bare_name_gets_no_edge_and_a_unique_one_is_a_guess() {
    let two = linked(&[
        ("x/a.py", "def work():\n    return 1\n"),
        ("y/b.py", "def work():\n    return 2\n"),
        ("z/c.py", "def run():\n    return work()\n"),
    ]);
    assert!(
        two.iter().all(|edge| !edge.starts_with("calls ")),
        "{two:?}"
    );
    let one = linked(&[
        ("x/a.py", "def work():\n    return 1\n"),
        ("z/c.py", "def run():\n    return work()\n"),
    ]);
    assert!(
        one.contains(&"calls s:z/c.py#run>s:x/a.py#work guess".to_string()),
        "{one:?}"
    );
}

#[test]
fn a_receiver_call_prefers_the_single_method_candidate() {
    let one = linked(&[(
        "b.ts",
        "export class Svc {\n  doIt(): number { return 1; }\n}\nconst o = { doIt() { return 2; } };\nexport function useDot(s: Svc) { return s.doIt(); }\n",
    )]);
    assert!(
        one.contains(&"calls s:b.ts#useDot>s:b.ts#Svc::doIt guess".to_string()),
        "{one:?}"
    );
    let two = linked(&[
        (
            "c/one.ts",
            "export class A { run(): number { return 1; } }\n",
        ),
        (
            "c/two.ts",
            "export class B { run(): number { return 2; } }\nexport function both(x: any) { return x.run(); }\n",
        ),
    ]);
    assert!(
        two.iter().all(|edge| !edge.starts_with("calls ")),
        "{two:?}"
    );
}

/// The first of several same-named object-literal members keeps its calls although the lexer numbers it `~1` and the index leaves it bare.
#[test]
fn the_first_duplicate_member_keeps_its_calls() {
    let source = "export function has(x: string): boolean { return !!x }\nexport const H = [\n  { detect: (p) =>\n      has('a') ||\n      has('b') },\n  { detect: (p) => has('c') },\n];\n";
    let edges = linked(&[("a.ts", source)]);
    assert!(
        edges.contains(&"calls s:a.ts#detect>s:a.ts#has name".to_string()),
        "{edges:?}"
    );
    assert!(
        edges.contains(&"calls s:a.ts#detect~2>s:a.ts#has name".to_string()),
        "{edges:?}"
    );
}

#[test]
fn a_bare_call_inside_a_python_method_reaches_the_imported_function_not_a_method() {
    let edges = linked(&[
        ("util.py", "def save():\n    return 1\n"),
        (
            "m.py",
            "from util import save\n\nclass A:\n    def save(self):\n        return save()\n\n    def run(self):\n        return self.save()\n",
        ),
    ]);
    assert!(
        edges.contains(&"calls s:m.py#A::save>s:util.py#save name".to_string()),
        "{edges:?}"
    );
    assert!(
        !edges
            .iter()
            .any(|edge| edge.starts_with("calls s:m.py#A::save>s:m.py#A::save")),
        "{edges:?}"
    );
    assert!(
        edges
            .iter()
            .any(|edge| edge.starts_with("calls s:m.py#A::run>s:m.py#A::save")),
        "{edges:?}"
    );
}
