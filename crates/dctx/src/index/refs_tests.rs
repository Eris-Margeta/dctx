// SPDX-License-Identifier: Apache-2.0
//! Tests for call and import linking over lexed fixtures.
//! Runs the lexers through `prepare_structural` (Rust through syn, as the walk does, when `syntax-rust` is on) and the
//! graph assembly the walk uses, then [`link`].
//! Each fixture mirrors one language of the DCX-12 card check.

use super::extract::{Prepared, index_prepared, prepare_structural};
use super::refs::{FileRefs, RawRef, link};
use super::{IndexedEdge, IndexedNode, TextBlob, write_endpoint};

/// Symbols and raw references of one file, the way the walk extracts them.
fn prepare(path: &str, source: &str) -> (Prepared, Vec<RawRef>) {
    #[cfg(feature = "syntax-rust")]
    if path.ends_with(".rs") {
        let (symbols, raw, _) =
            super::syn_extract::extract(path, source.as_bytes(), 1 << 20).expect("syn parses");
        return (Prepared::Symbols(symbols), raw);
    }
    prepare_structural(path, source.as_bytes())
}

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
        let (prepared, raw) = prepare(path, source);
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

/// A call resolves only to a symbol of the caller's language family: a Python call never reaches a shell function or a
/// Makefile target, while TypeScript still reaches JavaScript.
#[test]
fn calls_never_cross_into_another_language_family() {
    let edges = linked(&[
        ("legacy-c/Makefile", "all: build\n\nbuild:\n\tcc -o x x.c\n"),
        ("tools/run.sh", "deploy() {\n  echo hi\n}\n"),
        ("app/job.py", "def run():\n    deploy()\n    return all()\n"),
        ("web/util.js", "export function helper() { return 1; }\n"),
        (
            "web/main.ts",
            "export function start(): number { return helper(); }\n",
        ),
    ]);
    assert_stays_in_file(&edges, &["legacy-c/Makefile", "tools/run.sh"]);
    assert!(
        edges.contains(&"calls s:web/main.ts#start>s:web/util.js#helper guess".to_string()),
        "{edges:?}"
    );
}

/// Panics if an edge into one of `files` comes from another file.
fn assert_stays_in_file(edges: &[String], files: &[&str]) {
    let file_of = |id: &str| {
        id.get(2..)
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("")
            .to_string()
    };
    for edge in edges {
        let pair = edge.split(' ').nth(1).unwrap_or("");
        let (src, dst) = pair.split_once('>').unwrap_or(("", ""));
        for file in files {
            assert!(file_of(dst) != *file || file_of(src) == *file, "{edge}");
        }
    }
}

/// The dctx-public `legacy-c/Makefile#all` hub: Rust iterator `.all(..)` calls never reach a Makefile target, while a
/// lone Rust `all` in the same family is still a guess.
#[cfg(feature = "syntax-rust")]
#[test]
fn a_rust_iterator_call_never_reaches_a_makefile_target() {
    let edges = linked(&[
        ("legacy-c/Makefile", "all: build\n\nbuild:\n\tcc -o x x.c\n"),
        (
            "src/ledger.rs",
            "pub fn check(v: &[u8]) -> bool {\n    v.iter().all(|b| *b > 0)\n}\npub fn ship() {\n    build();\n}\n",
        ),
    ]);
    assert_stays_in_file(&edges, &["legacy-c/Makefile"]);
    let same = linked(&[
        ("src/a.rs", "pub fn all() -> bool {\n    true\n}\n"),
        ("src/b.rs", "pub fn go() -> bool {\n    all()\n}\n"),
    ]);
    assert!(
        same.contains(&"calls s:src/b.rs#go>s:src/a.rs#all guess".to_string()),
        "{same:?}"
    );
}

/// Without types a receiver call such as `v.iter().map(..)` must not guess its way to the one project method named
/// `map`: the pick survives only when the caller can see the owner (declares or imports it, or names it as qualifier).
#[cfg(feature = "syntax-rust")]
#[test]
fn a_receiver_call_reaches_a_project_method_only_when_its_owner_is_visible() {
    let session = (
        "src/session.rs",
        "pub struct Session;\nimpl Session {\n    pub fn map(&self) -> u8 {\n        1\n    }\n}\n",
    );
    let blind = linked(&[
        session,
        (
            "src/other.rs",
            "pub fn twice(v: &[u8]) -> Vec<u8> {\n    v.iter().map(|b| b * 2).collect()\n}\n",
        ),
    ]);
    assert!(
        blind.iter().all(|edge| !edge.starts_with("calls ")),
        "{blind:?}"
    );
    let want = "calls s:src/other.rs#twice>s:src/session.rs#Session::map";
    let imported = linked(&[
        session,
        (
            "src/other.rs",
            "use crate::session::Session;\npub fn twice(s: &Session) -> u8 {\n    s.map()\n}\n",
        ),
    ]);
    assert!(
        imported.iter().any(|edge| edge.starts_with(want)),
        "{imported:?}"
    );
    let qualified = linked(&[
        session,
        (
            "src/other.rs",
            "pub fn twice(s: &crate::session::Session) -> u8 {\n    Session::map(s)\n}\n",
        ),
    ]);
    assert!(
        qualified.iter().any(|edge| edge.starts_with(want)),
        "{qualified:?}"
    );
}
