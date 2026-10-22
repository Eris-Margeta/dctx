//! Oracles for Java unicode escapes, graph identity, hostile bounds and damaged containment.
//! The golden corpus never pinned these inputs; a control sits beside every refusal.
//! Calls the public extractor only.
#![forbid(unsafe_code)]

use dctx_lex_java::{
    Completion, DiagnosticReason, JavaLexer, LexError, LexedFile, Limits, Precision,
};

fn at(path: &str, limits: Limits, bytes: &[u8]) -> Result<LexedFile, LexError> {
    JavaLexer::new(limits).extract(path, bytes)
}

fn shape(facts: &LexedFile) -> (Completion, usize, Vec<String>, Vec<String>) {
    let nodes = facts
        .nodes
        .iter()
        .skip(1)
        .map(|node| format!("{} {}", node.fact.kind.as_str(), node.fact.name))
        .collect();
    let calls = facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind.as_str() == "calls")
        .map(|reference| reference.fact.name.clone())
        .collect();
    (facts.completion, facts.diagnostics.len(), nodes, calls)
}

fn assert_translates(escaped: &str, control: &str, nodes: &[&str], calls: &[&str]) {
    let limits = Limits::default();
    let got = at("A.java", limits, escaped.as_bytes()).expect("escaped");
    let ordinary = at("A.java", limits, control.as_bytes()).expect("control");
    assert_eq!(shape(&got), shape(&ordinary), "{escaped}");
    assert_eq!(ordinary.completion, Completion::Complete);
    assert_eq!(ordinary.diagnostics, []);
    let names: Vec<_> = ordinary.nodes.iter().skip(1).map(|node| node.fact.name.as_str()).collect();
    assert_eq!(names, nodes);
    let found: Vec<_> = ordinary
        .references
        .iter()
        .filter(|reference| reference.fact.kind.as_str() == "calls")
        .map(|reference| reference.fact.name.as_str())
        .collect();
    assert_eq!(found, calls);
    assert!(names.iter().all(|name| !name.contains("Ghost")));
}

#[test]
fn j1_unicode_escape_precision() {
    assert_translates(r"cl\u0061ss A {}", "class A {}", &["A"], &[]);
    assert_translates(
        r"\u002f\u002a class Ghost { void g(){ ghost(); } } \u002a\u002f class A { void live() { real(); } }",
        "/* class Ghost { void g(){ ghost(); } } */ class A { void live() { real(); } }",
        &["A", "A::live"],
        &["real"],
    );
    assert_translates(
        r"\u002f\u002f class Ghost {} \u000aclass A { void live() { real(); } }",
        "// class Ghost {} \nclass A { void live() { real(); } }",
        &["A", "A::live"],
        &["real"],
    );
    assert_translates(
        r#"class A { String s = "\u0063lass Ghost {}"; void live() { real(); } }"#,
        r#"class A { String s = "class Ghost {}"; void live() { real(); } }"#,
        &["A", "A::s", "A::live"],
        &["real"],
    );
}

const GRAPH: &str = r"
class Outer {
  void run() { one(); }
  void run(String value) { two(); }
  @Test void checks() {
    live();
    new Holder() {
      void go() {
        inner();
        new Nested() {
          void leaf() { deep(); }
        };
      }
    };
  }
}
";

fn node_ids(file: &LexedFile) -> Vec<&str> {
    file.nodes.iter().map(|node| node.fact.id.as_str()).collect()
}

fn call_srcs<'a>(facts: &'a LexedFile, name: &str) -> Vec<&'a str> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind.as_str() == "calls" && reference.fact.name == name)
        .map(|reference| reference.fact.src.as_str())
        .collect()
}

fn assert_call(facts: &LexedFile, name: &str, src: &str) {
    assert_eq!(call_srcs(facts, name), vec![src], "{name}");
}

fn assert_graph(path: &str, facts: &LexedFile) {
    assert_eq!(facts.path, path);
    assert_ne!(facts.nodes, []);
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    let mut ids: Vec<&str> = facts.nodes.iter().map(|node| node.fact.id.as_str()).collect();
    ids.sort_unstable();
    for pair in ids.windows(2) {
        assert_ne!(pair[0], pair[1], "duplicate id");
    }
    let mut destinations: Vec<&str> = facts.edges.iter().map(|edge| edge.fact.dst.as_str()).collect();
    destinations.sort_unstable();
    let mut nested: Vec<&str> = facts.nodes.iter().skip(1).map(|node| node.fact.id.as_str()).collect();
    nested.sort_unstable();
    assert_eq!(destinations, nested);
    let node = |id: &str| {
        facts
            .nodes
            .iter()
            .find(|candidate| candidate.fact.id.as_str() == id)
            .unwrap_or_else(|| panic!("missing {id}"))
    };
    for edge in &facts.edges {
        assert_eq!(edge.fact.owner_path, path);
        let parent = node(edge.fact.src.as_str());
        let child = node(edge.fact.dst.as_str());
        assert_eq!(child.fact.path, path);
        assert!(child.fact.lines[0] >= parent.fact.lines[0]);
        assert!(child.fact.lines[1] <= parent.fact.lines[1]);
        if parent.fact.kind.as_str() != "module" {
            assert!(child.fact.name.starts_with(&format!("{}::", parent.fact.name)));
        }
    }
    for reference in &facts.references {
        assert_eq!(reference.fact.owner_path, path);
        let source = node(reference.fact.src.as_str());
        assert!(reference.fact.lines[0] >= source.fact.lines[0], "{}", reference.fact.name);
        assert!(reference.fact.lines[1] <= source.fact.lines[1], "{}", reference.fact.name);
    }
}

#[test]
fn j2_generated_graph_ownership() {
    // Not a defect: overload ordinals, nested anonymous owners and @Test call
    // sources already belong to the output graph. This is the missing oracle.
    let facts = at("O.java", Limits::default(), GRAPH.as_bytes()).expect("graph");
    assert_eq!(facts.completion, Completion::Complete);
    assert_graph("O.java", &facts);
    let simple = at("S.java", Limits::default(), b"class A { void m() { live(); } }").expect("simple");
    assert_graph("S.java", &simple);
    assert_call(&simple, "live", "s:S.java#A::m");
    assert_call(&facts, "one", "s:O.java#Outer::run~1");
    assert_call(&facts, "two", "s:O.java#Outer::run~2");
    assert_call(&facts, "live", "t:O.java#Outer::checks");
    assert_call(&facts, "new Holder", "t:O.java#Outer::checks");
    assert_call(&facts, "inner", "s:O.java#Outer::checks::new Holder::go");
    assert_call(&facts, "new Nested", "s:O.java#Outer::checks::new Holder::go");
    assert_call(&facts, "deep", "s:O.java#Outer::checks::new Holder::go::new Nested::leaf");
    let tests = |name: &str| {
        facts.references.iter().any(|reference| {
            reference.fact.kind.as_str() == "tests" && reference.fact.name == name
        })
    };
    assert!(tests("deep"));
    assert!(!tests("one"));
    let shifted = at("O.java", Limits::default(), format!("\n\n{GRAPH}").as_bytes()).expect("shift");
    assert_eq!(node_ids(&facts), node_ids(&shifted));
    assert_graph("O.java", &shifted);
}

fn padded(prefix: &[u8], suffix: &[u8]) -> Vec<u8> {
    let ceiling = Limits::CEILING.max_bytes;
    let mut source = Vec::with_capacity(ceiling);
    source.extend_from_slice(prefix);
    source.resize(ceiling - suffix.len(), b' ');
    source.extend_from_slice(suffix);
    assert_eq!(source.len(), ceiling);
    source
}

fn quotes(lines: usize) -> Vec<u8> {
    std::iter::repeat_n(b"\"\n", lines).flatten().copied().collect()
}

#[test]
fn j3_hostile_token_and_diagnostic_bounds() {
    // Not a defect: byte, depth and fact refusals are already exact, and retained
    // diagnostics already stop at LexedFile::MAX_DIAGNOSTICS.
    assert_eq!(Limits::CEILING.max_bytes, 8 << 20);
    assert_eq!(Limits::CEILING.max_depth, 256);
    assert_eq!(LexedFile::MAX_DIAGNOSTICS, 64);
    let mut wide = padded(b"class A {/*", b"*/}");
    let accepted = at("A.java", Limits::default(), &wide).expect("ceiling");
    assert_eq!(accepted.completion, Completion::Complete);
    assert_eq!(accepted.diagnostics, []);
    assert!(accepted.nodes.iter().any(|node| node.fact.name == "A"));
    wide.push(b' ');
    assert_eq!(
        at("A.java", Limits::default(), &wide),
        Err(LexError::TooLarge {
            bytes: Limits::CEILING.max_bytes + 1,
            limit: Limits::CEILING.max_bytes,
        })
    );
    wide.pop();
    for (limit, ok) in [(0, false), (1, false), (2, true)] {
        let limits = Limits { max_facts: limit, ..Limits::default() };
        let result = at("A.java", limits, &wide);
        if ok {
            let facts = result.expect("facts fit");
            assert!(facts.nodes.iter().any(|node| node.fact.name == "A"));
        } else {
            assert_eq!(result, Err(LexError::TooManyFacts { limit }));
        }
    }
    let mut head = vec![b'('; 300];
    head.extend_from_slice(b"/*");
    assert_eq!(
        at("A.java", Limits::default(), &padded(&head, b"*/")),
        Err(LexError::TooDeep { line: 1, limit: Limits::CEILING.max_depth })
    );
    let depth = Limits { max_depth: 4, ..Limits::default() };
    assert_eq!(
        at("A.java", depth, b"class A {\n(\n(\n(\n(\n"),
        Err(LexError::TooDeep { line: 5, limit: 4 })
    );
    let shallow = at("A.java", depth, b"class A {\n(\n(\n(\n)\n)\n)\n}\n").expect("shallow");
    assert_eq!(shallow.completion, Completion::Complete);
    assert!(shallow.nodes.iter().any(|node| node.fact.name == "A"));
    let busy = format!("class A {{ void m() {{ {} }} }}", "a();".repeat(4096));
    let few = Limits { max_facts: 3, ..Limits::default() };
    assert_eq!(at("A.java", few, busy.as_bytes()), Err(LexError::TooManyFacts { limit: 3 }));
    let method = at("A.java", few, b"class A { void m() {} }").expect("method");
    assert!(method.nodes.iter().any(|node| node.fact.name == "A::m"));
    let busy_ok = at("A.java", Limits::default(), busy.as_bytes()).expect("busy");
    assert!(busy_ok.references.iter().any(|reference| reference.fact.name == "a"));
    let cap = LexedFile::MAX_DIAGNOSTICS;
    let damaged = at("A.java", Limits::default(), &quotes(cap + 1)).expect("damaged");
    assert_eq!(damaged.completion, Completion::Partial);
    assert_eq!(damaged.diagnostics.len(), cap);
    assert_eq!(damaged.diagnostics[0].byte, 0);
    assert_eq!(damaged.diagnostics[cap - 1].byte, u32::try_from((cap - 1) * 2).expect("bounded test coordinate"));
    assert!(damaged.diagnostics.iter().all(|d| d.reason == DiagnosticReason::UnterminatedString));
    assert_eq!(at("A.java", Limits::default(), &quotes(cap)).unwrap().diagnostics.len(), cap);
    let kept = at("A.java", Limits::default(), b"class A {}\n\"\n").expect("one");
    assert_eq!(kept.diagnostics.len(), 1);
    assert_eq!(kept.completion, Completion::Partial);
    assert_eq!(kept.diagnostics[0].reason, DiagnosticReason::UnterminatedString);
    assert!(kept.nodes.iter().any(|node| node.fact.name == "A"));
    let clean = at("A.java", Limits::default(), b"class A {}").expect("clean");
    assert_eq!(clean.completion, Completion::Complete);
    assert_eq!(clean.diagnostics, []);
}

#[test]
fn j4_review_unicode_escapes_in_raw_source() {
    // Not a defect: JLS `\u` translation already runs before token classification.
    // A keyword may start with an escape, and a line comment ends at a `\u` newline
    // (including a `\\u` run). The unescaped source is the positive control.
    assert_translates(r"\u0063lass A {}", "class A {}", &["A"], &[]);
    assert_translates(
        r"// c\u000a class B {}",
        "// c\n class B {}",
        &["B"],
        &[],
    );
    assert_translates(
        r"// c\\u000a class C {}",
        "// c\\\n class C {}",
        &["C"],
        &[],
    );
}

fn containment<'a>(facts: &'a LexedFile, name: &str) -> (Precision, &'a str) {
    let node = facts
        .nodes
        .iter()
        .find(|candidate| candidate.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name}"));
    let edge = facts
        .edges
        .iter()
        .find(|edge| edge.fact.dst == node.fact.id)
        .unwrap_or_else(|| panic!("missing edge {name}"));
    (edge.precision, edge.fact.conf.as_str())
}

#[test]
fn j5_review_partial_contains_stays_exact() {
    // Not a defect: a contains edge past the first diagnostic is already
    // Precision::Partial. EdgeKind::Contains permits only Confidence::Exact,
    // so the confidence label stays exact while damage lives on precision.
    let damaged = r#"
class Half {
    void ok() { ready(); }
    void later() {
        String s = "unterminated
        laterCall();
    }
}
"#;
    let facts = at("A.java", Limits::default(), damaged.as_bytes()).expect("damaged");
    assert_eq!(facts.completion, Completion::Partial);
    assert_ne!(facts.diagnostics, []);
    assert_eq!(containment(&facts, "Half::ok"), (Precision::Structural, "exact"));
    assert_eq!(containment(&facts, "Half"), (Precision::Partial, "exact"));
    assert_eq!(containment(&facts, "Half::later"), (Precision::Partial, "exact"));
    let clean = at(
        "A.java",
        Limits::default(),
        b"class Half { void ok() { ready(); } }",
    )
    .expect("clean");
    assert_eq!(clean.completion, Completion::Complete);
    assert_eq!(clean.diagnostics, []);
    assert_eq!(containment(&clean, "Half"), (Precision::Structural, "exact"));
    assert_eq!(containment(&clean, "Half::ok"), (Precision::Structural, "exact"));
}
