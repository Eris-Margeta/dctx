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
        let end = u32::try_from(nodes.len()).expect("small");
        if !raw.is_empty() || end > first + 1 {
            spans.push(FileRefs {
                first,
                end,
                refs: raw,
            });
        }
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

/// Non-containment edges from the production walk, including the confidence of each edge.
fn walked_dependencies(files: &[(&str, &str)]) -> Vec<[String; 4]> {
    use super::{WalkArgs, walk_source};
    use crate::domain::{
        FreshnessMode, HostGrants, OperationLimits, ParserChoice, PolicyView, resolve_scan,
    };
    use dctx_core::selection::{RuleTexts, SelectionPolicy};
    use dctx_ports::memory::{CancelFlag, MemorySource};
    use dctx_template::HostCeiling;

    let mut source = MemorySource::new();
    for (path, body) in files {
        source
            .insert_file(path, body.as_bytes(), 0)
            .expect("fixture file");
    }
    let walked = walk_source(&WalkArgs {
        source: &source,
        cancel: &CancelFlag::new(),
        policy: PolicyView {
            selection: &SelectionPolicy::from_texts(&RuleTexts::default()),
            ceiling: &HostCeiling::base(),
            grants: HostGrants::PermitAll,
            plan: None,
            view: "map",
        },
        limits: &resolve_scan(&OperationLimits::default()).expect("limits"),
        freshness: FreshnessMode::ImmutableSnapshot,
        parser: ParserChoice::Structural,
        deadline_ms: None,
        started_ms: 0,
        now_ms: 0,
        elapsed: None,
    })
    .expect("walk");

    let mut dependencies = Vec::new();
    for edge in &walked.edges {
        let kind = walked.text.get(edge.edge);
        if kind == "contains" {
            continue;
        }
        let (mut src, mut dst) = (String::new(), String::new());
        write_endpoint(&mut src, &walked.nodes, &walked.text, edge.src);
        write_endpoint(&mut dst, &walked.nodes, &walked.text, edge.dst);
        dependencies.push([
            src,
            kind.to_owned(),
            dst,
            walked.text.get(edge.confidence).to_owned(),
        ]);
    }
    dependencies.sort();
    dependencies
}

#[test]
fn walk_imports_modules_regardless_of_contents_or_decoys() {
    for (importer, import, target, decoy, comment, declaration) in [
        (
            "p/b.mjs",
            "import './util.mjs';",
            "p/util.mjs",
            "q/util.mjs",
            "// side effects only",
            "export function hit() {}",
        ),
        (
            "b.py",
            "import util",
            "util.py",
            "q/util.py",
            "# side effects only",
            "def hit():\n    pass\n",
        ),
    ] {
        for body in ["", comment, declaration] {
            for with_decoy in [false, true] {
                let mut files = vec![(importer, import), (target, body)];
                if with_decoy {
                    files.push((decoy, declaration));
                }
                assert_eq!(
                    walked_dependencies(&files),
                    [[
                        format!("m:{importer}"),
                        "imports".into(),
                        format!("m:{target}"),
                        "exact".into()
                    ]],
                    "{files:?}"
                );
            }
        }
    }
}

#[test]
fn qualified_parameter_calls_need_evidence_beyond_the_module_filename() {
    for receiver in ["util", "renamed"] {
        for stem in ["util", "renamed"] {
            let module = format!("unrelated/{stem}.py");
            let caller = format!("def run({receiver}):\n    return {receiver}.hit()\n");
            let edges =
                walked_dependencies(&[("b.py", &caller), (&module, "def hit():\n    pass\n")]);
            assert!(edges.is_empty(), "{receiver:?} {module:?} {edges:?}");

            let module = format!("{stem}.mjs");
            let caller = format!(
                "import './{module}';\nexport function run({receiver}) {{ {receiver}.hit(); }}"
            );
            let files = [
                ("b.mjs", caller.as_str()),
                (&module, "export function hit() {}"),
            ];
            assert_eq!(
                walked_dependencies(&files),
                [[
                    "m:b.mjs".to_owned(),
                    "imports".into(),
                    format!("m:{module}"),
                    "exact".into()
                ]],
                "{files:?}"
            );
        }
    }
}

#[test]
fn namespace_aliases_resolve_qualified_calls_independently_of_the_filename() {
    for (importer, caller, module, definition) in [
        (
            "b.mjs",
            "import * as dep from './util.mjs';\nexport function run() { dep.hit(); }",
            "util.mjs",
            "export function hit() {}",
        ),
        (
            "b.py",
            "import util as dep\ndef run():\n    return dep.hit()\n",
            "util.py",
            "def hit():\n    pass\n",
        ),
    ] {
        assert_eq!(
            walked_dependencies(&[(importer, caller), (module, definition)]),
            [
                [
                    format!("m:{importer}"),
                    "imports".into(),
                    format!("m:{module}"),
                    "exact".into()
                ],
                [
                    format!("s:{importer}#run"),
                    "calls".into(),
                    format!("s:{module}#hit"),
                    "name".into()
                ],
            ]
        );
    }
}

#[test]
fn local_namespace_alias_does_not_reach_a_sibling_scope() {
    let edges = walked_dependencies(&[
        (
            "b.py",
            "def left():\n    import util as dep\n    dep.hit()\ndef left_more(dep):\n    dep.hit()\n",
        ),
        ("util.py", "def hit():\n    pass\n"),
    ]);
    assert_eq!(
        edges,
        [
            ["s:b.py#left", "calls", "s:util.py#hit", "name"],
            ["s:b.py#left", "imports", "m:util.py", "exact"],
        ]
    );
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
fn js_imports_with_spelled_extensions_bind_named_and_default_calls() {
    let edges = linked(&[
        (
            "util.js",
            "export function helper() {}\nexport default function () {}\n",
        ),
        (
            "main.js",
            "import { helper } from './util.js';\nimport d from './util.js';\nexport function run() { helper(); d(); }\n",
        ),
    ]);
    for edge in [
        "imports m:main.js>s:util.js#helper exact",
        "imports m:main.js>s:util.js#default exact",
        "calls s:main.js#run>s:util.js#helper name",
        "calls s:main.js#run>s:util.js#default name",
    ] {
        assert!(edges.iter().any(|got| got == edge), "{edge:?} in {edges:?}");
    }
}

#[test]
fn js_relative_imports_resolve_in_the_importers_directory_only() {
    let target = ("p/lib/x.js", "export function f() {}");
    for (importer, specifier) in [("p/b.js", "./lib/x"), ("p/sub/b.js", "../lib/x")] {
        let source = format!("import {{ f }} from '{specifier}';\n");
        for other in ["lib/x.js", "q/lib/x.js"] {
            let edges = linked(&[(other, target.1), target, (importer, &source)]);
            let imports: Vec<_> = edges
                .iter()
                .filter(|edge| edge.starts_with("imports "))
                .collect();
            assert_eq!(
                imports,
                [&format!("imports m:{importer}>s:p/lib/x.js#f exact")]
            );
        }
    }
    for (importer, specifier) in [
        ("z/b.js", "./lib/x"),
        ("z/sub/b.js", "../lib/x"),
        ("b.js", "../p/lib/x"),
    ] {
        let source = format!("import {{ f }} from '{specifier}';\n");
        let edges = linked(&[target, (importer, &source)]);
        assert!(
            edges.iter().all(|edge| !edge.starts_with("imports ")),
            "{edges:?}"
        );
    }
}

#[test]
fn relative_imports_try_spelled_path_then_extensions_then_index() {
    let edges = linked(&[
        ("p/data", ""),
        ("p/data.js", ""),
        ("p/x.ts", ""),
        ("p/x.ts.js", ""),
        ("p/y.tsx", ""),
        ("p/y/index.js", ""),
        ("p/pkg/index.mts", ""),
        (
            "p/main.ts",
            "import './data';\nimport './x.ts';\nimport './y';\nimport './sub/../pkg';\n",
        ),
    ]);
    let imports: Vec<_> = edges
        .iter()
        .filter(|edge| edge.starts_with("imports "))
        .map(String::as_str)
        .collect();
    assert_eq!(
        imports,
        [
            "imports m:p/main.ts>m:p/data exact",
            "imports m:p/main.ts>m:p/x.ts exact",
            "imports m:p/main.ts>m:p/y.tsx exact",
            "imports m:p/main.ts>m:p/pkg/index.mts exact",
        ]
    );
}

#[test]
fn bare_js_imports_still_use_module_suffixes() {
    let edges = linked(&[
        ("vendor/scope/pkg.js", "export function f() {}"),
        ("p/main.ts", "import { f } from '@scope/pkg';\n"),
    ]);
    assert!(
        edges
            .iter()
            .any(|edge| edge == "imports m:p/main.ts>s:vendor/scope/pkg.js#f exact"),
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
        edges.contains(&"calls s:a.ts#H::detect>s:a.ts#has name".to_string()),
        "{edges:?}"
    );
    assert!(
        edges.contains(&"calls s:a.ts#H::detect~2>s:a.ts#has name".to_string()),
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
