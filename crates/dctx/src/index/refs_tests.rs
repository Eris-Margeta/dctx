// SPDX-License-Identifier: Apache-2.0
//! Tests for call and import linking over lexed fixtures.
//! Runs the lexers through `prepare_structural` (Rust through syn, as the walk does, when `syntax-rust` is on) and the
//! graph assembly the walk uses, then [`link`].
//! Each fixture mirrors one language of the DCX-12 card check.

use super::extract::{Prepared, index_prepared, prepare_structural};
use super::refs::{FileRefs, ImportPaths, RawRef, link_with_paths};
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
    linked_with(files, ImportPaths::default())
}

fn linked_with(files: &[(&str, &str)], paths: ImportPaths<'_>) -> Vec<String> {
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
    link_with_paths(&nodes, &mut edges, &mut text, &spans, paths);
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
fn js_unproven_members_link_the_file_without_binding_calls() {
    for clause in ["dep", "{ hidden as dep }", "{ absent as dep }"] {
        let importer =
            format!("import {clause} from './a.js'; export function run() {{ dep(); dep.f(); }}");
        assert_eq!(
            walked_dependencies(&[
                ("a.js", "export function f() {} function hidden() {}"),
                ("b.js", &importer),
                ("decoy.js", "export function dep() {}"),
            ]),
            [["m:b.js", "imports", "m:a.js", "exact"]],
            "{clause:?}"
        );
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
fn bare_js_imports_require_package_ownership() {
    let edges = linked(&[
        ("vendor/scope/pkg.js", "export function f() {}"),
        ("p/main.ts", "import { f } from '@scope/pkg';\n"),
    ]);
    assert!(
        edges.iter().all(|edge| !edge.starts_with("imports ")),
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
        ("src/lib.rs", "mod session; mod other;"),
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

#[test]
fn javascript_reexports_follow_relative_edges_through_barrels() {
    for suffix in ["", ".js"] {
        for (clause, target) in [
            ("{ f }", "s:p/a.js#f"),
            ("{ f as g }", "s:p/a.js#f"),
            ("*", "m:p/a.js"),
            ("* as ns", "m:p/a.js"),
        ] {
            let barrel = format!("export {clause} from './a{suffix}';");
            let files = [
                ("a.js", "export function f() {}"),
                ("p/a.js", "export function f() {}"),
                ("p/b.js", barrel.as_str()),
                ("p/c.js", "export * from './b';"),
                ("q/b.js", barrel.as_str()),
            ];
            assert_eq!(
                walked_dependencies(&files),
                [
                    ["m:p/b.js", "reexports", target, "exact"],
                    ["m:p/c.js", "reexports", "m:p/b.js", "exact"]
                ],
                "{files:?}"
            );
        }
    }
}

#[test]
fn javascript_literal_dependencies_and_calls_preserve_spelling() {
    let files = [
        (
            "p/main.mjs",
            r"import 'node:util'; import 'util'; import '@scope/nope'; import 'missing';
import './a-b.mjs'; import './a.b.mjs'; import './\u0061.mjs?v=1#tag'; import './util';
export function use(path) { import('./lazy.js'); import(path); }",
        ),
        ("p/a-b.mjs", ""),
        ("p/a/b.mjs", ""),
        ("p/a.b.mjs", ""),
        ("p/a.mjs", ""),
        ("p/util.js", ""),
        ("p/lazy.js", ""),
        ("node/util.js", ""),
        ("missing.js", ""),
    ];
    assert_eq!(
        walked_dependencies(&files),
        [
            ["m:p/main.mjs", "imports", "m:p/a-b.mjs", "exact"],
            ["m:p/main.mjs", "imports", "m:p/a.b.mjs", "exact"],
            ["m:p/main.mjs", "imports", "m:p/a.mjs", "exact"],
            ["m:p/main.mjs", "imports", "m:p/util.js", "exact"],
            ["s:p/main.mjs#use", "imports", "m:p/lazy.js", "exact"]
        ]
    );
}

#[test]
fn js_members_use_the_exported_identifier_before_file_names() {
    let a = "export function $hit() {} export function hit() {} export function run() { $hit(); hit(); }";
    for decoy in [false, true] {
        let mut files = vec![
            ("a.ts", a),
            (
                "b.ts",
                "import { $hit as x, hit as y } from './a'; export function run() {x(); y();}",
            ),
        ];
        if decoy {
            files.push(("a/hit.ts", "export function other() {}"));
        }
        assert_eq!(
            walked_dependencies(&files),
            [
                ["m:b.ts", "imports", "s:a.ts#$hit", "exact"],
                ["m:b.ts", "imports", "s:a.ts#hit", "exact"],
                ["s:a.ts#run", "calls", "s:a.ts#$hit", "name"],
                ["s:a.ts#run", "calls", "s:a.ts#hit", "name"],
                ["s:b.ts#run", "calls", "s:a.ts#$hit", "name"],
                ["s:b.ts#run", "calls", "s:a.ts#hit", "name"]
            ]
        );
    }
}

#[test]
fn js_named_default_private_and_type_exports_control_runtime_bindings() {
    for (declaration, name) in [
        ("function hit() {}", "hit"),
        ("function() {}", "default"),
        ("class Hit {}", "Hit"),
        ("class {}", "default"),
    ] {
        let body = format!("export default {declaration}");
        let edges = walked_dependencies(&[
            ("a.ts", &body),
            ("b.ts", "import work from './a'; function run(){work();}"),
        ]);
        assert_eq!(edges.len(), 2, "{edges:?}");
        assert!(
            edges.iter().all(|e| e[2] == format!("s:a.ts#{name}")),
            "{edges:?}"
        );
    }
    for source in [
        "import type {Item} from './a';",
        "import {type Item} from './a';",
    ] {
        let body =
            format!("{source} declare const Item: () => number; function run() {{ Item(); }}");
        assert_eq!(
            walked_dependencies(&[
                ("a.ts", "export type Item = () => number;"),
                ("b.ts", &body)
            ]),
            [["m:b.ts", "imports", "s:a.ts#Item", "exact"]]
        );
    }
    for keyword in ["", "export "] {
        let a =
            format!("{keyword}function f() {{}} function local() {{}} export {{local as api}};");
        let edges = walked_dependencies(&[
            ("a.ts", &a),
            (
                "b.ts",
                "import {f, api} from './a'; function run(){ f(); api();}",
            ),
        ]);
        assert_eq!(
            edges.iter().filter(|e| e[2] == "s:a.ts#f").count(),
            if keyword.is_empty() { 0 } else { 2 },
            "{edges:?}"
        );
        assert_eq!(
            edges.iter().filter(|e| e[2] == "s:a.ts#local").count(),
            2,
            "{edges:?}"
        );
    }
}

#[test]
fn vue_type_only_imports_keep_dependencies_without_runtime_calls() {
    for imports in [
        "import type {work as Typed} from './lib'; import {run} from './lib';",
        "import {type work as Typed, run} from './lib';",
    ] {
        let component = format!(
            "<template>\n\x1b\"\n</template>\n<script setup lang=\"ts\">\n{imports}\nfunction use() {{ Typed(); run(); }}\n</script>"
        );
        assert_eq!(
            walked_dependencies(&[
                ("ui/Card.vue", &component),
                (
                    "ui/lib.ts",
                    "export function work() {} export function run() {}"
                ),
            ]),
            [
                ["m:ui/Card.vue", "imports", "s:ui/lib.ts#run", "exact"],
                ["m:ui/Card.vue", "imports", "s:ui/lib.ts#work", "exact"],
                ["s:ui/Card.vue#use", "calls", "s:ui/lib.ts#run", "name"],
            ],
            "{imports:?}"
        );
    }
}

#[test]
fn js_loader_bindings_are_lexical() {
    let files = [
        ("a.cjs", ""),
        ("other.cjs", ""),
        (
            "b.cjs",
            "function load(require){return require('./other.cjs');} require('./a.cjs');",
        ),
        ("c.cjs", "function require(x){} require('./other.cjs');"),
        (
            "arrow.cjs",
            "const a = require => require('./other.cjs'); const b = (require) => require('./other.cjs');",
        ),
    ];
    let got = walked_dependencies(&files);
    let imports: Vec<_> = got.iter().filter(|e| e[1] == "imports").collect();
    assert_eq!(imports.len(), 1, "{got:?}");
    assert_eq!(imports[0][2], "m:a.cjs");
}

#[test]
fn script_assignment_exports_bind_only_proven_callable_values() {
    let main = "const run = require('./a.cjs'); function use(){run();}";
    for (body, call) in [
        ("function hit() {} module.exports = hit;", true),
        ("function hit() {} module.exports = {hit};", false),
        (
            "function hit() {} module.exports = hit; if (x) module.exports = other;",
            false,
        ),
    ] {
        let edges = walked_dependencies(&[("a.cjs", body), ("b.cjs", main)]);
        assert!(
            edges.iter().any(|e| e[1] == "imports" && e[2] == "m:a.cjs"),
            "{edges:?}"
        );
        assert_eq!(
            edges
                .iter()
                .any(|e| e[1] == "calls" && e[2] == "s:a.cjs#hit"),
            call,
            "{edges:?}"
        );
    }
    let edges = walked_dependencies(&[
        ("a.ts", "export function hit() {}"),
        (
            "b.ts",
            "import work = require('./a'); export function run(){work.hit();}",
        ),
    ]);
    assert_eq!(
        edges,
        [
            ["m:b.ts", "imports", "m:a.ts", "exact"],
            ["s:b.ts#run", "calls", "s:a.ts#hit", "name"]
        ]
    );
}

#[test]
fn python_module_operands_and_package_levels_survive_lookup() {
    for name in ["main", "index", "mod", "lib", "h"] {
        let target = format!("p/{name}.py");
        let body = format!(
            "import static.tools\nfrom .{name} import hit\nimport p.h\nfrom ...bad import x\nimport stray\n"
        );
        let mut files = vec![
            ("p/b.py", body.as_str()),
            ("p/__init__.py", ""),
            ("static/__init__.py", ""),
            ("static/tools.py", ""),
            ("tools.py", ""),
            ("p.h", ""),
            ("other/stray.py", ""),
            (&target, "def hit(): pass"),
        ];
        if name != "h" {
            files.push(("p/h.py", ""));
        }
        let edges = walked_dependencies(&files);
        assert_eq!(edges.len(), 3, "{edges:?}");
        for dst in ["m:static/tools.py", "m:p/h.py", &format!("s:{target}#hit")] {
            assert!(edges.iter().any(|e| e[2] == dst), "{edges:?}");
        }
    }
    let files = [
        ("p/__init__.py", ""),
        ("p/sub/__init__.py", ""),
        ("p/util.py", "def hit(): pass"),
        ("util.py", "def hit(): pass"),
        ("p/sub/b.py", "from ..util import hit"),
        ("p/b.py", "from . import util\ndef run(): return util.hit()"),
    ];
    assert_eq!(
        walked_dependencies(&files),
        [
            ["m:p/b.py", "imports", "m:p/util.py", "exact"],
            ["m:p/sub/b.py", "imports", "s:p/util.py#hit", "exact"],
            ["s:p/b.py#run", "calls", "s:p/util.py#hit", "name"]
        ]
    );
}

#[test]
fn python_lexical_imports_wildcards_and_package_members() {
    for decoy in [false, true] {
        let mut files = vec![
            ("p/__init__.py", "def hit(): pass"),
            ("b.py", "from p import hit\ndef run(): hit()"),
        ];
        if decoy {
            files.push(("p/hit.py", "def other(): pass"));
        }
        assert_eq!(
            walked_dependencies(&files),
            [
                ["m:b.py", "imports", "s:p/__init__.py#hit", "exact"],
                ["s:b.py#run", "calls", "s:p/__init__.py#hit", "name"]
            ]
        );
    }
    for (list, exported) in [("['hit']", true), ("[]", false), ("['hit'] + extra", false)] {
        let a = format!("__all__ = {list}\ndef hit(): pass\ndef hidden(): pass");
        let edges = walked_dependencies(&[
            ("a.py", &a),
            ("c.py", "def hit(): pass\ndef hidden(): pass"),
            ("b.py", "from a import *\ndef run(): hit(); hidden()"),
        ]);
        assert_eq!(
            edges.iter().filter(|e| e[1] == "calls").count(),
            usize::from(exported),
            "{edges:?}"
        );
        assert!(edges.iter().any(|e| e[2] == "m:a.py"), "{edges:?}");
    }
    for order in [false, true] {
        let mut funcs = [
            "def left():\n from a import hit as go\n go()\n",
            "def right():\n from c import hit as go\n go()\n",
        ];
        if order {
            funcs.reverse();
        }
        let b = format!(
            "from a import hit as go\n{}{}def unknown(go): go()\n",
            funcs[0], funcs[1]
        );
        let edges = walked_dependencies(&[
            ("a.py", "def hit(): pass"),
            ("c.py", "def hit(): pass"),
            ("b.py", &b),
        ]);
        for (src, dst) in [
            ("s:b.py#left", "s:a.py#hit"),
            ("s:b.py#right", "s:c.py#hit"),
        ] {
            assert!(
                edges
                    .iter()
                    .any(|e| e[0] == src && e[1] == "calls" && e[2] == dst && e[3] == "name"),
                "{edges:?}"
            );
        }
        assert!(!edges.iter().any(|e| e[0] == "s:b.py#unknown"), "{edges:?}");
    }
}

#[cfg(feature = "syntax-rust")]
#[test]
fn rust_module_declarations_anchor_use_paths_and_overrides() {
    for parent in ["p.rs", "p/mod.rs"] {
        let files = [
            ("lib.rs", "mod p; #[path = \"actual.rs\"] mod item;"),
            (
                parent,
                "pub mod util; pub mod b; mod inline { use super::super::p::util::hit; } use self::util::hit;",
            ),
            ("p/util.rs", "pub fn hit(){}"),
            ("p/b.rs", "use super::util::hit; pub fn run(){hit();}"),
            ("util.rs", "pub fn hit(){}"),
            ("item.rs", ""),
            ("actual.rs", ""),
        ];
        let edges = linked(&files);
        for want in [
            format!("imports m:{parent}>m:p/util.rs exact"),
            format!("imports m:{parent}>s:p/util.rs#hit exact"),
            "imports m:lib.rs>m:actual.rs exact".into(),
            "imports m:p/b.rs>s:p/util.rs#hit exact".into(),
            "calls s:p/b.rs#run>s:p/util.rs#hit name".into(),
        ] {
            assert!(edges.contains(&want), "{want:?} {edges:?}");
        }
        assert!(
            !edges.iter().any(
                |e| e.contains(">m:item.rs exact") && e.starts_with("imports")
                    || e.contains(">s:util.rs#hit") && e.starts_with("imports")
            ),
            "{edges:?}"
        );
    }
}

#[cfg(feature = "syntax-rust")]
#[test]
fn rust_public_aliases_link_consumers_with_visibility_and_cycle_checks() {
    for (vis, exported) in [("pub ", true), ("", false), ("pub(crate) ", false)] {
        let barrel = format!("mod inner; mod client; {vis}use crate::inner::hit as api;");
        let edges = linked(&[
            ("lib.rs", &barrel),
            ("inner.rs", "pub fn hit(){}"),
            ("client.rs", "use crate::api; fn run(){api();}"),
        ]);
        assert_eq!(
            edges
                .iter()
                .any(|e| e == "reexports m:lib.rs>s:inner.rs#hit exact"),
            exported,
            "{edges:?}"
        );
        for edge in [
            "imports m:client.rs>s:inner.rs#hit exact",
            "calls s:client.rs#run>s:inner.rs#hit name",
        ] {
            assert!(edges.iter().any(|e| e == edge), "{edges:?}");
        }
    }
}

#[test]
fn go_packages_use_module_ownership_and_declared_binding_across_files() {
    for (import, call) in [("", "tools.Hit()"), ("work ", "work.Hit()"), ("_ ", "")] {
        let main = format!(
            "package main\nimport {import}\"audit/tools\"\nimport \"fmt\"\nfunc main(){{ {call}; fmt.Println(1) }}"
        );
        let files = [
            ("go.mod", "module audit\n"),
            ("cmd/main.go", &main),
            ("tools/impl.go", "package tools\nfunc Hit(){}"),
            ("tools/more.go", "package tools\nfunc More(){}"),
            ("audit/tools.go", "package wrong\nfunc Other(){}"),
            ("unused/fmt.go", "package unused\nfunc Println(i int){}"),
        ];
        let edges = walked_dependencies(&files);
        assert_eq!(
            edges.iter().filter(|e| e[1] == "imports").count(),
            2,
            "{edges:?}"
        );
        assert!(edges.iter().all(|e| e[2].contains(":tools/")), "{edges:?}");
        assert_eq!(
            edges
                .iter()
                .any(|e| e[1] == "calls" && e[2] == "s:tools/impl.go#Hit" && e[3] == "name"),
            !call.is_empty(),
            "{edges:?}"
        );
    }
}

#[test]
fn jvm_imports_follow_declared_packages_and_static_members() {
    for src in ["src/p/Widget.java", "moved/Widget.java"] {
        let files = [
            (
                src,
                "package p; public class Widget { public static int hit(){return 1;} }",
            ),
            (
                "p/Widget.java",
                "package other; public class Widget { public static int hit(){return 2;} }",
            ),
            (
                "Main.java",
                "import p.Widget; import static p.Widget.hit; class Main { int run(){return hit();} }",
            ),
            ("Main.kt", "import p.work as local\nfun run()=local()"),
            ("Util.kt", "package p\nfun work()=1"),
            ("More.kt", "package p\nfun extra()=2"),
        ];
        let edges = walked_dependencies(&files);
        for dst in [
            format!("s:{src}#Widget"),
            format!("s:{src}#Widget::hit"),
            "s:Util.kt#work".into(),
        ] {
            assert!(
                edges.iter().any(|e| e[1] == "imports" && e[2] == dst),
                "{edges:?}"
            );
        }
        assert_eq!(
            edges
                .iter()
                .filter(|e| e[1] == "calls" && e[3] == "name")
                .count(),
            2,
            "{edges:?}"
        );
    }
}

#[test]
fn namespace_and_sdk_imports_ignore_same_named_files() {
    for path in ["Types.cs", "src/Move.cs"] {
        let edges = walked_dependencies(&[
            (path, "namespace P; public class Widget {}"),
            ("P/Widget.cs", "namespace Other { public class Widget {} }"),
            ("Use.cs", "using W = P.Widget; class A {}"),
            ("Use.cpp", "namespace mylib {} using namespace mylib;"),
            ("mylib.cpp", "int x;"),
            ("Use.swift", "import Foundation"),
            ("Foundation.swift", "struct Decoy {}"),
        ]);
        assert_eq!(
            edges,
            [[
                String::from("m:Use.cs"),
                "imports".into(),
                format!("s:{path}#P::Widget"),
                "exact".into()
            ]]
        );
    }
}

#[test]
fn swift_import_requires_declared_dependency_and_target_membership() {
    let files = [
        (
            "Package.swift",
            r#"import PackageDescription
let package = Package(name: "Example", targets: [.target(name: "Core"), .executableTarget(name: "App", dependencies: ["Core"])])"#,
        ),
        ("Sources/App/Main.swift", "import Core\nimport Foundation"),
        ("Sources/Core/First.swift", "struct A {}"),
        ("Sources/Core/Other.swift", "struct B {}"),
        ("Foundation.swift", ""),
    ];
    assert_eq!(
        walked_dependencies(&files),
        [
            [
                "m:Sources/App/Main.swift",
                "imports",
                "m:Sources/Core/First.swift",
                "exact"
            ],
            [
                "m:Sources/App/Main.swift",
                "imports",
                "m:Sources/Core/Other.swift",
                "exact"
            ]
        ]
    );
}

#[test]
fn php_namespace_uses_and_files_have_independent_context() {
    let files = [
        (
            "p/app.php",
            r"<?php
use P\{function hit as work, Thing as T};
work();
require './missing/util.php';
include __DIR__ . '/defs.php';",
        ),
        ("p/defs.php", ""),
        (
            "real.php",
            "<?php namespace P; function hit() {} class Thing {}",
        ),
        ("P/hit.php", "<?php namespace Other; class hit {}"),
        ("unrelated/missing/util.php", ""),
        ("missing/util.php", ""),
    ];
    for cwd in [Some(""), None] {
        let edges = linked_with(
            &files,
            ImportPaths {
                cwd,
                ..Default::default()
            },
        );
        for want in [
            "imports m:p/app.php>s:real.php#P::hit exact",
            "imports m:p/app.php>s:real.php#P::Thing exact",
            "imports m:p/app.php>m:p/defs.php exact",
            "calls m:p/app.php>s:real.php#P::hit name",
        ] {
            assert!(edges.iter().any(|e| e == want), "{edges:?}");
        }
        assert_eq!(
            edges
                .iter()
                .any(|e| e == "imports m:p/app.php>m:missing/util.php exact"),
            cwd.is_some(),
            "{edges:?}"
        );
    }
}

#[test]
fn header_search_obeys_delimiters_extensions_and_include_order() {
    let files = [
        (
            "p/x.cpp",
            "#include \"item.h\"\n#include <item.h>\n#include <vector>\n#include \"api.h\"",
        ),
        ("p/item.h", ""),
        ("root1/item.h", ""),
        ("root2/item.h", ""),
        ("item.h", ""),
        ("p/api.h", ""),
        ("p/api.c", ""),
        ("vendor/vector.cpp", ""),
    ];
    for roots in [["root1", "root2"], ["root2", "root1"]] {
        let edges = linked_with(
            &files,
            ImportPaths {
                includes: &roots,
                ..Default::default()
            },
        );
        let imports: Vec<_> = edges.iter().filter(|e| e.starts_with("imports ")).collect();
        assert_eq!(imports.len(), 3, "{edges:?}");
        for path in ["p/item.h", "p/api.h", &format!("{}/item.h", roots[0])] {
            assert!(
                imports.contains(&&format!("imports m:p/x.cpp>m:{path} exact")),
                "{edges:?}"
            );
        }
    }
}

#[test]
fn ruby_loads_keep_the_loader_and_anchor() {
    let files = [
        (
            "p/sub/x.rb",
            "require_relative '../util'\nrequire_relative 'absent'\nrequire 'util'\nload 'exact.rb'",
        ),
        ("p/util.rb", ""),
        ("lib/util.rb", ""),
        ("lib/absent.rb", ""),
        ("util.rb", ""),
        ("lib/exact.rb", ""),
    ];
    let edges = linked_with(
        &files,
        ImportPaths {
            ruby: &["lib"],
            ..Default::default()
        },
    );
    let imports: Vec<_> = edges.iter().filter(|e| e.starts_with("imports ")).collect();
    assert_eq!(imports.len(), 3, "{edges:?}");
    for path in ["p/util.rb", "lib/util.rb", "lib/exact.rb"] {
        assert!(
            imports.contains(&&format!("imports m:p/sub/x.rb>m:{path} exact")),
            "{edges:?}"
        );
    }
}

#[test]
fn config_extends_binds_only_the_manifest_field_and_local_file() {
    for present in [true, false] {
        let mut files = vec![
            (
                "p/tsconfig.json",
                r#"{"extends":"./base.json","other":"./base.json"}"#,
            ),
            ("base.json", "{}"),
        ];
        if present {
            files.push(("p/base.json", "{}"));
        }
        let got = walked_dependencies(&files);
        assert_eq!(got.len(), usize::from(present), "{got:?}");
        if present {
            assert_eq!(
                got,
                [[
                    "s:p/tsconfig.json#extends",
                    "imports",
                    "m:p/base.json",
                    "exact"
                ]]
            );
        }
    }
}

#[test]
fn markdown_documents_are_relative_and_keep_heading_owners() {
    let got = walked_dependencies(&[
        (
            "sub/guide.md",
            "# Guide\n\n[Notes](notes.md) [One](notes.md#one) [web](data:text/plain,notes.md) `notes.md`\n",
        ),
        ("sub/notes.md", "# One\n"),
        ("notes.md", "# Decoy\n"),
    ]);
    assert_eq!(
        got,
        [
            [
                "d:sub/guide.md#Guide",
                "documents",
                "d:sub/notes.md#One",
                "name"
            ],
            [
                "d:sub/guide.md#Guide",
                "documents",
                "m:sub/notes.md",
                "name"
            ]
        ]
    );
}

#[test]
fn proto_imports_require_literal_directives_and_ordered_roots() {
    let files = [
        (
            "b.proto",
            r#"syntax = "proto3"; import public "types.proto"; import weak "weak.proto";
option java_package = 'import "decoy.proto";'; // import "missing.proto";
import "absent.proto";"#,
        ),
        ("one/types.proto", ""),
        ("two/types.proto", ""),
        ("one/weak.proto", ""),
        ("one/decoy.proto", ""),
    ];
    for roots in [["one", "two"], ["two", "one"]] {
        let edges = linked_with(
            &files,
            ImportPaths {
                proto: &roots,
                ..Default::default()
            },
        );
        assert_eq!(
            edges.iter().filter(|e| e.starts_with("imports ")).count(),
            2,
            "{edges:?}"
        );
        assert!(
            edges.contains(&format!(
                "imports m:b.proto>m:{}/types.proto exact",
                roots[0]
            )),
            "{edges:?}"
        );
    }
}

#[test]
fn lua_loader_requires_a_literal_search_path_and_standard_binding() {
    for (setup, body, hit) in [
        (
            "package.path = \"./?.lua\"",
            "local m = require(\"a\")",
            true,
        ),
        ("package.path = runtime", "local m = require(\"a\")", false),
        (
            "package.path = \"./?.lua\"",
            "local function load(require)\nlocal m = require(\"a\")\nend",
            false,
        ),
        (
            "package.path = \"./?.lua\"",
            "package.searchers = {}\nlocal m = require(\"a\")",
            false,
        ),
        (
            "package.path = \"./?.lua\"",
            "local m = require(name)",
            false,
        ),
    ] {
        let source = format!("{setup}\n{body}\nreturn m\n");
        let got = walked_dependencies(&[("b.lua", &source), ("a.lua", "return {}")]);
        assert_eq!(got.len(), usize::from(hit), "{got:?}");
        if hit {
            assert_eq!(got, [["m:b.lua", "imports", "m:a.lua", "exact"]]);
        }
    }
}

#[test]
fn ts_export_assignment_requires_default_interop() {
    for (value, enabled) in [("true", true), ("false", false), (r#""true""#, false)] {
        let config =
            format!(r#"{{"compilerOptions":{{"module":"commonjs","esModuleInterop":{value}}}}}"#);
        let got = walked_dependencies(&[
            ("a.ts", "function hit() {}\nexport = hit;"),
            ("b.ts", "import work from './a'; function run(){work();}"),
            ("tsconfig.json", &config),
        ]);
        if enabled {
            assert_eq!(
                got,
                [
                    ["m:b.ts", "imports", "s:a.ts#hit", "exact"],
                    ["s:b.ts#run", "calls", "s:a.ts#hit", "name"],
                ]
            );
        } else {
            assert_eq!(got, [["m:b.ts", "imports", "m:a.ts", "exact"]]);
        }
    }
}

#[test]
fn literal_module_diagnostics_escape_hostile_values() {
    let hostile = "./name\n\x1b\".js";
    let message = format!("{hostile:?}");
    assert_eq!(message.lines().count(), 1);
    assert!(!message.contains('\x1b'));
    assert!(message.contains("\\\""));
    let got = walked_dependencies(&[
        ("main.mjs", r#"import './name\n\x1b".js';"#),
        ("name.js", ""),
    ]);
    assert!(got.is_empty(), "{message} {got:?}");
}

#[test]
fn typescript_assignment_chain_and_separate_name_spaces() {
    let got = walked_dependencies(&[
        ("a.ts", "function hit(){} export = hit;"),
        (
            "b.ts",
            "import work = require('./a'); function use(){work();}",
        ),
    ]);
    assert_eq!(
        got,
        [
            ["m:b.ts", "imports", "m:a.ts", "exact"],
            ["s:b.ts#use", "calls", "s:a.ts#hit", "name"]
        ]
    );
    let got = walked_dependencies(&[
        (
            "a.ts",
            "export type Item = () => number; export function Item(){return 1;}",
        ),
        (
            "b.ts",
            "import {type Item as T, Item as value} from './a'; function run(){T();value();}",
        ),
    ]);
    assert_eq!(
        got,
        [
            ["m:b.ts", "imports", "s:a.ts#Item", "exact"],
            ["m:b.ts", "imports", "s:a.ts#Item~2", "exact"],
            ["s:b.ts#run", "calls", "s:a.ts#Item~2", "name"]
        ]
    );
}

#[test]
fn named_consumers_follow_public_barrel_aliases() {
    let got = walked_dependencies(&[
        ("a.js", "export function f(){}"),
        ("b.js", "export { f as api } from './a';"),
        (
            "c.js",
            "import {api as go} from './b.js'; function run(){go();}",
        ),
    ]);
    assert_eq!(
        got,
        [
            ["m:b.js", "reexports", "s:a.js#f", "exact"],
            ["m:c.js", "imports", "s:a.js#f", "exact"],
            ["s:c.js#run", "calls", "s:a.js#f", "name"]
        ]
    );
}

#[test]
fn python_dotted_bindings_and_unknown_wildcards_keep_their_owners() {
    let edges = walked_dependencies(&[
        (
            "b.py",
            "import p.tools\nfrom maybe import *\ndef run():\n p.tools.go()\n absent()",
        ),
        ("p/__init__.py", ""),
        ("p/tools.py", "def go(): pass"),
        ("maybe.py", "__all__ = make_names()\ndef absent(): pass"),
    ]);
    assert!(
        edges
            .iter()
            .any(|e| e[1] == "calls" && e[2] == "s:p/tools.py#go"),
        "{edges:?}"
    );
    assert!(
        !edges
            .iter()
            .any(|e| e[1] == "calls" && e[2] == "s:maybe.py#absent"),
        "{edges:?}"
    );
}

#[test]
fn php_directory_magic_constant_requires_an_expression() {
    let edges = walked_dependencies(&[
        ("p/main.php", r#"<?php include "__DIR__ . '/defs.php'";"#),
        ("p/defs.php", ""),
    ]);
    assert!(edges.is_empty(), "{edges:?}");
}

#[cfg(feature = "syntax-rust")]
#[test]
fn rust_alias_cycles_and_private_sibling_aliases_abstain() {
    let edges = linked(&[
        ("lib.rs", "mod a; mod b;"),
        (
            "a.rs",
            "pub use crate::b::looped as cycle; use crate::b::hit as hidden;",
        ),
        (
            "b.rs",
            "pub use crate::a::cycle as looped; use crate::a::hidden; pub fn hit(){}",
        ),
    ]);
    assert!(
        !edges
            .iter()
            .any(|e| e.starts_with("reexports ") || e == "imports m:b.rs>s:b.rs#hit exact"),
        "{edges:?}"
    );
}

#[test]
fn python_import_roots_bound_absolute_and_relative_paths() {
    let edges = linked_with(
        &[
            (
                "src/p/b.py",
                "import mod
from . import util
from ..outside import nope",
            ),
            ("src/p/__init__.py", ""),
            ("src/p/util.py", ""),
            ("src/mod.py", ""),
            ("mod.py", ""),
            ("outside.py", "def nope(): pass"),
        ],
        ImportPaths {
            python: "src",
            ..Default::default()
        },
    );
    assert!(
        edges
            .iter()
            .any(|e| e == "imports m:src/p/b.py>m:src/mod.py exact"),
        "{edges:?}"
    );
    assert!(
        edges
            .iter()
            .any(|e| e == "imports m:src/p/b.py>m:src/p/util.py exact"),
        "{edges:?}"
    );
    assert_eq!(
        edges.iter().filter(|e| e.starts_with("imports ")).count(),
        2,
        "{edges:?}"
    );
}

#[test]
fn cpp_using_directives_never_select_translation_units() {
    for decoy in [None, Some("mylib.cpp"), Some("renamed.cpp")] {
        let mut files = vec![("src/main.cpp", "namespace mylib {} using namespace mylib;")];
        files.extend(decoy.map(|path| (path, "int value;")));
        let edges = walked_dependencies(&files);
        assert!(edges.is_empty(), "{edges:?}");
    }
}
