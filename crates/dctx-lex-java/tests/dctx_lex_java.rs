//! Golden fixtures, refusal controls and deterministic fuzz for the Java extractor.
//! Implements PARSERS-v1 validation: partial facts, typed refusals and stable output.
//! Uses the public `JavaLexer` API only.
#![forbid(unsafe_code)]

use dctx_lex_java::{
    body_fingerprint, Completion, DiagnosticReason, Dialect, JavaLexer, LexError, LexedFile,
    Limits, Precision,
};

const BASIC: &str = include_str!("fixtures/basic.java");
const INCOMPLETE: &str = include_str!("fixtures/incomplete.java");

struct Expect {
    name: &'static str,
    kind: &'static str,
    symbol: &'static str,
    public: bool,
    precision: Precision,
    signature: &'static str,
    doc: Option<&'static str>,
}

fn expect(
    name: &'static str,
    kind: &'static str,
    symbol: &'static str,
    public: bool,
    signature: &'static str,
    doc: Option<&'static str>,
) -> Expect {
    Expect {
        name,
        kind,
        symbol,
        public,
        precision: Precision::Structural,
        signature,
        doc,
    }
}

#[test]
fn basic_fixture_matches_the_golden_facts() {
    let facts = JavaLexer::default()
        .extract("src/sample.java", BASIC.as_bytes())
        .expect("basic");
    assert_eq!(facts.completion, Completion::Complete);
    assert!(facts.diagnostics.is_empty());
    assert_eq!(facts.dialect, Dialect::Java);
    assert_eq!(facts.path, "src/sample.java");
    let module = &facts.nodes[0];
    assert_eq!(module.fact.kind.as_str(), "module");
    assert_eq!(module.fact.id.as_str(), "m:src/sample.java");
    assert_eq!(module.fact.name, "src/sample.java");
    assert_eq!(module.fact.signature, "com.example.app");
    assert_eq!(module.fact.summary.text, "Sample types for café.");
    assert_eq!(module.fact.summary.source.as_str(), "doc");
    assert!(!module.fact.public);
    assert_eq!(module.precision, Precision::Structural);
    assert_eq!(module.fact.fingerprint, body_fingerprint(BASIC));

    let expected = [
        expect("Box", "symbol", "class", true, "public class Box<T extends CharSequence> extends Base implements Runnable", Some("A box of values.")),
        expect("Box::capacity", "symbol", "const", true, "public final int capacity", Some("Capacity.")),
        expect("Box::names", "symbol", "const", false, "private List<String> names", None),
        expect("Box::Box", "symbol", "method", true, "public Box(int capacity)", Some("Builds a box.")),
        expect("Box::run", "symbol", "method", true, "@Override public void run()", None),
        expect("Box::Nested", "symbol", "class", true, "public static class Nested", Some("Nested holder.")),
        expect("Box::Nested::ping", "symbol", "method", false, "void ping()", None),
        expect("Box::Nested::ping", "symbol", "method", false, "void ping(int times)", None),
        expect("Point", "symbol", "struct", false, "record Point(int x, String label) implements Marker", Some("One point.")),
        expect("Point::x", "symbol", "const", false, "int x", None),
        expect("Point::label", "symbol", "const", false, "String label", None),
        expect("Point::doubled", "symbol", "method", true, "public int doubled()", None),
        expect("Color", "symbol", "enum", false, "enum Color", None),
        expect("Color::RED", "symbol", "const", true, "RED", None),
        expect("Color::GREEN", "symbol", "const", true, "GREEN", None),
        expect("Color::BLUE", "symbol", "const", true, "BLUE", None),
        expect("Color::BLUE::code", "symbol", "method", false, "int code()", None),
        expect("Color::n", "symbol", "const", false, "private final int n", None),
        expect("Color::Color", "symbol", "method", false, "Color(int n)", None),
        expect("Task", "symbol", "interface", false, "interface Task", None),
        expect("Task::run", "symbol", "method", true, "void run()", None),
        expect("Marker", "symbol", "interface", false, "@FunctionalInterface interface Marker", None),
        expect("Tag", "symbol", "interface", false, "@interface Tag", None),
        expect("Tag::value", "symbol", "method", true, "String value()", None),
        expect("App", "symbol", "class", false, "class App", None),
        expect("App::greeting", "symbol", "const", false, "private String greeting", None),
        expect("App::mark", "symbol", "const", false, "private char mark", None),
        expect("App::tricky", "symbol", "const", false, "private String tricky", None),
        expect("App::start", "symbol", "method", false, "void start()", None),
        expect("App::start::new Runnable", "symbol", "class", false, "new Runnable()", None),
        expect("App::start::new Runnable::run", "symbol", "method", true, "public void run()", None),
        Expect {
            name: "App::saves",
            kind: "test",
            symbol: "method",
            public: false,
            precision: Precision::Convention,
            signature: "@Test void saves()",
            doc: None,
        },
    ];
    assert_eq!(facts.nodes.len(), expected.len() + 1);
    let mut ping = 1u32;
    for (node, expect) in facts.nodes.iter().skip(1).zip(expected) {
        assert_eq!(node.fact.name, expect.name, "name");
        assert_eq!(node.fact.kind.as_str(), expect.kind, "{}", expect.name);
        assert_eq!(
            node.fact.symbol_kind.map(|kind| kind.as_str()),
            Some(expect.symbol),
            "{}",
            expect.name
        );
        assert_eq!(node.fact.public, expect.public, "{}", expect.name);
        assert_eq!(node.precision, expect.precision, "{}", expect.name);
        assert_eq!(node.fact.signature, expect.signature, "{}", expect.name);
        match expect.doc {
            Some(text) => {
                assert_eq!(node.fact.summary.source.as_str(), "doc", "{}", expect.name);
                assert_eq!(node.fact.summary.text, text, "{}", expect.name);
            }
            None => {
                assert_eq!(node.fact.summary.source.as_str(), "sig", "{}", expect.name);
                assert_eq!(node.fact.summary.text, expect.signature, "{}", expect.name);
            }
        }
        if expect.name == "Box::Nested::ping" {
            assert_eq!(
                node.fact.id.as_str(),
                format!("s:src/sample.java#Box::Nested::ping~{ping}")
            );
            ping += 1;
        }
        if expect.name == "App::saves" {
            assert_eq!(node.fact.id.as_str(), "t:src/sample.java#App::saves");
        }
        if expect.name == "Box::capacity" {
            assert_eq!(node.fact.fingerprint, body_fingerprint("2"));
        }
    }
    assert_parent(&facts, "Box::Nested::ping", "Box::Nested");
    assert_parent(&facts, "Color::BLUE::code", "Color::BLUE");
    assert_parent(&facts, "Point::label", "Point");
    assert_parent(&facts, "App::start::new Runnable", "App::start");
    assert_parent(&facts, "App::start::new Runnable::run", "App::start::new Runnable");
    assert_parent(&facts, "App::saves", "App");

    let imports = named(&facts, "imports");
    assert_eq!(
        imports,
        [
            "java.util.List",
            "java.util.*",
            "static java.util.Collections.emptyList",
            "static java.util.Objects.*"
        ]
    );
    assert!(facts.references.iter().filter(|r| r.fact.kind.as_str() == "imports").all(|r| {
        r.fact.alias.is_none() && r.fact.scope.is_empty() && r.precision == Precision::Structural
    }));

    let calls: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.fact.kind.as_str() == "calls")
        .map(|r| (r.fact.name.as_str(), r.fact.scope.as_str(), r.precision))
        .collect();
    assert_eq!(
        calls,
        [
            ("store.load", "Box::Box", Precision::Structural),
            ("service.run", "Box::run", Precision::Structural),
            ("emptyList", "Box::run", Precision::Structural),
            ("pingBack", "Box::Nested::ping", Precision::Structural),
            ("pingBack", "Box::Nested::ping", Precision::Structural),
            ("new Runnable", "App::start", Precision::Structural),
            ("hook", "App::start::new Runnable::run", Precision::Structural),
            ("names.forEach", "App::start", Precision::Structural),
            ("sink.accept", "App::start", Precision::Structural),
            ("new Point", "App::start", Precision::Structural),
            ("Store.find", "App::start", Precision::Structural),
            ("repo.save", "App::saves", Precision::Structural),
        ]
    );
    let ping_src: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.fact.name == "pingBack")
        .map(|r| r.fact.src.as_str())
        .collect();
    assert_eq!(
        ping_src,
        [
            "s:src/sample.java#Box::Nested::ping~1",
            "s:src/sample.java#Box::Nested::ping~2"
        ]
    );
    let tests: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.fact.kind.as_str() == "tests")
        .map(|r| (r.fact.name.as_str(), r.fact.scope.as_str(), r.precision))
        .collect();
    assert_eq!(tests, [("repo.save", "App::saves", Precision::Convention)]);

    let implements = scoped(&facts, "implements");
    assert_eq!(implements, [("Runnable", "Box"), ("Marker", "Point")]);
    let uses = scoped(&facts, "uses-type");
    assert_eq!(
        uses,
        [
            ("CharSequence", "Box"),
            ("Base", "Box"),
            ("List", "Box::names"),
            ("String", "Box::names"),
            ("Override", "Box::run"),
            ("String", "Point::label"),
            ("FunctionalInterface", "Marker"),
            ("String", "Tag::value"),
            ("String", "App::greeting"),
            ("String", "App::tricky"),
            ("Test", "App::saves"),
        ]
    );
    for banned in ["NotAClass", "Fake", "Ghost", "point.x", "String::trim", "Consumer"] {
        assert!(facts.nodes.iter().all(|n| !n.fact.name.contains(banned)), "{banned}");
        assert!(
            facts.references.iter().all(|r| r.fact.name != banned),
            "{banned}"
        );
    }
    assert!(facts.nodes.iter().all(|n| n.fact.name != "no" && !n.fact.name.ends_with("::no")));
    invariants(BASIC, &facts);
    let again = JavaLexer::default().extract("src/sample.java", BASIC.as_bytes()).expect("again");
    assert_eq!(facts, again);
}

#[test]
fn incomplete_file_keeps_partial_facts() {
    let facts = JavaLexer::default()
        .extract("src/incomplete.java", INCOMPLETE.as_bytes())
        .expect("partial");
    assert_eq!(facts.completion, Completion::Partial);
    assert_eq!(facts.diagnostics.len(), 1);
    assert_eq!(facts.diagnostics[0].reason, DiagnosticReason::UnterminatedString);
    let quote = INCOMPLETE.find("\"unterminated").expect("quote") as u32;
    assert_eq!(facts.diagnostics[0].byte, quote);
    assert_eq!(facts.nodes[0].fact.signature, "demo");
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    let names: Vec<_> = facts.nodes.iter().skip(1).map(|n| n.fact.name.as_str()).collect();
    assert_eq!(names, ["Half", "Half::ok", "Half::later"]);
    assert_eq!(precision_of(&facts, "Half"), Precision::Partial);
    assert_eq!(precision_of(&facts, "Half::ok"), Precision::Structural);
    assert_eq!(precision_of(&facts, "Half::later"), Precision::Partial);
    let ready = facts.references.iter().find(|r| r.fact.name == "ready").expect("ready");
    let later = facts.references.iter().find(|r| r.fact.name == "laterCall").expect("later");
    assert_eq!(ready.precision, Precision::Structural);
    assert_eq!(ready.fact.kind.as_str(), "calls");
    assert_eq!(later.precision, Precision::Partial);
    assert_eq!(later.fact.kind.as_str(), "calls");
    invariants(INCOMPLETE, &facts);
}

#[test]
fn unclosed_brace_is_partial_not_fatal() {
    let facts = JavaLexer::default()
        .extract("A.java", b"class A { void m() {")
        .expect("unclosed");
    assert_eq!(facts.completion, Completion::Partial);
    assert!(facts.diagnostics.iter().any(|d| d.reason == DiagnosticReason::UnbalancedBracket));
}

#[test]
fn limits_refuse_with_positive_controls() {
    let deep = "(".repeat(5000);
    assert!(matches!(
        JavaLexer::default().extract("A.java", deep.as_bytes()),
        Err(LexError::TooDeep { .. })
    ));
    let mut nested = String::from("class A { ");
    for _ in 0..300 {
        nested.push_str("List<");
    }
    nested.push_str("String");
    for _ in 0..300 {
        nested.push('>');
    }
    nested.push_str(" x; }");
    assert!(matches!(
        JavaLexer::default().extract("A.java", nested.as_bytes()),
        Err(LexError::TooDeep { .. })
    ));
    let limited = JavaLexer::new(Limits {
        max_depth: 8,
        ..Limits::default()
    });
    assert!(limited.extract("A.java", b"class A<T> { List<String> x; }").is_ok());
    assert!(limited.extract("A.java", b"(())").is_ok());
    let four = JavaLexer::new(Limits {
        max_depth: 4,
        ..Limits::default()
    });
    assert!(four.extract("A.java", b"(((())))").is_ok());
    assert!(matches!(
        four.extract("A.java", b"((((("),
        Err(LexError::TooDeep { limit: 4, .. })
    ));
    let zero_depth = JavaLexer::new(Limits {
        max_depth: 0,
        ..Limits::default()
    });
    assert!(zero_depth.extract("A.java", b"").is_ok());
    assert!(matches!(
        zero_depth.extract("A.java", b"("),
        Err(LexError::TooDeep { .. })
    ));
    let two = JavaLexer::new(Limits {
        max_facts: 2,
        ..Limits::default()
    });
    assert!(two.extract("A.java", b"class A {}").is_ok());
    assert!(matches!(
        two.extract("A.java", b"class A { void m() {} }"),
        Err(LexError::TooManyFacts { limit: 2 })
    ));
    let one = JavaLexer::new(Limits {
        max_facts: 1,
        ..Limits::default()
    });
    assert!(one.extract("A.java", b"").is_ok());
    assert!(matches!(
        one.extract("A.java", b"class A {}"),
        Err(LexError::TooManyFacts { limit: 1 })
    ));
    let zero = JavaLexer::new(Limits {
        max_facts: 0,
        ..Limits::default()
    });
    assert!(matches!(
        zero.extract("A.java", b""),
        Err(LexError::TooManyFacts { limit: 0 })
    ));
}

#[test]
fn truncated_prefixes_and_arbitrary_bytes_do_not_panic() {
    prefixes("src/sample.java", BASIC);
    prefixes("src/incomplete.java", INCOMPLETE);
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..40 {
        let len = (xorshift(&mut state) as usize) % 301;
        let mut buf = vec![0_u8; len];
        for byte in &mut buf {
            *byte = xorshift(&mut state) as u8;
        }
        accept("fuzz.java", &buf);
    }
    accept("fuzz.java", b"class A { void m() { a.b(); } }");
    accept("fuzz.java", b"\"\"\"\n");
    accept("fuzz.java", b"/*");
    accept("fuzz.java", b"class A<T> {}");
    accept("fuzz.java", &vec![b'{'; 300]);
    accept("fuzz.java", &[b'('; 100]);
    accept("fuzz.java", &[0xff, 0xfe, b'c', b'l', b'a', b's', b's']);
}

fn prefixes(path: &str, source: &str) {
    let bytes = source.as_bytes();
    for len in 0..=bytes.len() {
        let slice = &bytes[..len];
        let once = JavaLexer::default().extract(path, slice);
        let twice = JavaLexer::default().extract(path, slice);
        match (&once, &twice) {
            (Ok(left), Ok(right)) => {
                assert_eq!(left, right);
                let text = std::str::from_utf8(slice).expect("utf8");
                invariants(text, left);
            }
            (Err(left), Err(right)) => {
                assert_eq!(left, right);
                match left {
                    LexError::InvalidUtf8(err) => assert!(err.valid_up_to() <= slice.len()),
                    LexError::TooDeep { .. }
                    | LexError::TooManyFacts { .. }
                    | LexError::TooLarge { .. } => {}
                    other => panic!("unexpected refusal {other}"),
                }
            }
            _ => panic!("nondeterministic at {len}"),
        }
    }
}

fn accept(path: &str, bytes: &[u8]) {
    let once = JavaLexer::default().extract(path, bytes);
    let twice = JavaLexer::default().extract(path, bytes);
    match (&once, &twice) {
        (Ok(left), Ok(right)) => {
            assert_eq!(left, right);
            let text = std::str::from_utf8(bytes).expect("utf8");
            invariants(text, left);
        }
        (Err(left), Err(right)) => {
            assert_eq!(left, right);
            assert!(matches!(
                left,
                LexError::InvalidUtf8(_) | LexError::TooDeep { .. } | LexError::TooManyFacts { .. }
            ));
        }
        _ => panic!("nondeterministic fuzz"),
    }
}

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn invariants(source: &str, facts: &LexedFile) {
    let lines = source.lines().count().max(1) as u32;
    assert!(!facts.nodes.is_empty());
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert_eq!(facts.nodes[0].fact.kind.as_str(), "module");
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    if facts.diagnostics.is_empty() {
        assert_eq!(facts.completion, Completion::Complete);
    } else {
        assert_eq!(facts.completion, Completion::Partial);
    }
    for node in &facts.nodes {
        assert!(node.fact.lines[0] >= 1 && node.fact.lines[0] <= lines, "{}", node.fact.name);
        assert!(node.fact.lines[1] >= node.fact.lines[0] && node.fact.lines[1] <= lines);
        assert_eq!(node.fact.fingerprint.len(), 64, "{}", node.fact.name);
        assert!(node.fact.fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
    }
    for edge in &facts.edges {
        assert_eq!(edge.fact.kind.as_str(), "contains");
        assert_eq!(edge.fact.conf.as_str(), "exact");
    }
    for reference in &facts.references {
        assert_ne!(reference.fact.kind.as_str(), "contains");
        assert!(!reference.fact.name.is_empty());
        assert!(reference.fact.alias.is_none());
        assert!(reference.fact.lines[0] >= 1 && reference.fact.lines[0] <= lines);
        assert!(reference.fact.lines[1] >= reference.fact.lines[0] && reference.fact.lines[1] <= lines);
    }
}

fn named<'a>(facts: &'a LexedFile, kind: &str) -> Vec<&'a str> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind.as_str() == kind)
        .map(|reference| reference.fact.name.as_str())
        .collect()
}

fn scoped<'a>(facts: &'a LexedFile, kind: &str) -> Vec<(&'a str, &'a str)> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind.as_str() == kind)
        .map(|reference| (reference.fact.name.as_str(), reference.fact.scope.as_str()))
        .collect()
}

fn precision_of(facts: &LexedFile, name: &str) -> Precision {
    facts
        .nodes
        .iter()
        .find(|node| node.fact.name == name)
        .expect(name)
        .precision
}

fn assert_parent(facts: &LexedFile, child: &str, parent: &str) {
    let child_node = facts.nodes.iter().find(|node| node.fact.name == child).expect(child);
    let edge = facts
        .edges
        .iter()
        .find(|edge| edge.fact.dst == child_node.fact.id)
        .expect("edge");
    let src = facts
        .nodes
        .iter()
        .find(|node| node.fact.id == edge.fact.src)
        .expect("parent");
    assert_eq!(src.fact.name, parent, "{child}");
}
