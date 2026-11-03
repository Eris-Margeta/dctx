// SPDX-License-Identifier: Apache-2.0
//! Golden fixtures, store ownership and deterministic fuzz for the C# extractor.
//! Implements PARSERS-v1 validation: partial facts, typed refusals and stable output.
//! Uses the public `CsLexer` API only.
#![forbid(unsafe_code)]

use dctx_lex_cs::{
    body_fingerprint, Completion, CsLexer, DiagnosticReason, Dialect, LexError, LexedFile, Precision,
};
use std::collections::BTreeSet;

const BASIC: &str = include_str!("fixtures/basic.cs");
const INCOMPLETE: &str = include_str!("fixtures/incomplete.cs");

struct Row {
    name: &'static str,
    symbol: &'static str,
    public: bool,
    signature: &'static str,
    doc: Option<&'static str>,
}

fn row(
    name: &'static str,
    symbol: &'static str,
    public: bool,
    signature: &'static str,
    doc: Option<&'static str>,
) -> Row {
    Row { name, symbol, public, signature, doc }
}

#[test]
#[allow(clippy::too_many_lines, reason = "The golden fixture is one expected declaration table and its invariant checks; splitting it would separate expected records from their verification.")]
fn basic_fixture_matches_the_golden_facts() {
    let facts = CsLexer::default().extract("src/App.cs", BASIC.as_bytes()).expect("basic");
    assert_eq!(facts.completion, Completion::Complete);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    assert_eq!(facts.dialect, Dialect::CSharp);
    assert_eq!(facts.path, "src/App.cs");
    assert_eq!(dctx_lex_cs::PARSER_ID, "dctx-lex-cs");
    assert_eq!(dctx_lex_cs::PARSER_REVISION, 1);
    let module = &facts.nodes[0];
    assert_eq!(module.fact.kind.as_str(), "module");
    assert_eq!(module.fact.id.as_str(), "m:src/App.cs");
    assert_eq!(module.fact.name, "src/App.cs");
    assert_eq!(module.fact.signature, "App.Sample");
    assert_eq!(module.fact.summary.text, "Sample types for café.");
    assert_eq!(module.fact.summary.source.as_str(), "doc");
    assert!(!module.fact.public);
    assert_eq!(module.precision, Precision::Structural);
    assert_eq!(module.fact.fingerprint, body_fingerprint(BASIC));

    let expected = [
        row("App.Sample", "", true, "namespace App.Sample", None),
        row("App.Sample::Box", "class", true, "public partial class Box<T> : Base, IDisposable where T : class", Some("A box of values.")),
        row("App.Sample::Box::capacity", "const", true, "public int capacity", Some("Capacity.")),
        row("App.Sample::Box::names", "const", false, "private List<string> names", None),
        row("App.Sample::Box::Name", "const", true, "public string Name", None),
        row("App.Sample::Box::Box", "method", true, "public Box(int capacity)", Some("Builds a box.")),
        row("App.Sample::Box::Run", "method", true, "public void Run()", None),
        row("App.Sample::Box::DebugOnly", "method", true, "public void DebugOnly()", None),
        row("App.Sample::Box::Nested", "class", true, "public static class Nested", None),
        row("App.Sample::Box::Nested::Ping", "method", false, "void Ping()", None),
        row("App.Sample::Point", "struct", true, "public record Point(int X, string Label)", Some("One point.")),
        row("App.Sample::Point::X", "const", false, "int X", None),
        row("App.Sample::Point::Label", "const", false, "string Label", None),
        row("App.Sample::Point::Doubled", "method", true, "public int Doubled()", None),
        row("App.Sample::Color", "enum", true, "public enum Color", None),
        row("App.Sample::Color::Red", "const", true, "Red", None),
        row("App.Sample::Color::Green", "const", true, "Green", None),
        row("App.Sample::Color::Blue", "const", true, "Blue", None),
        row("App.Sample::ITask", "interface", true, "public interface ITask", None),
        row("App.Sample::ITask::Run", "method", true, "void Run()", None),
        row("App.Sample::Holder", "struct", true, "[Obsolete] public struct Holder", None),
        row("App.Sample::Holder::Value", "const", true, "public int Value", None),
    ];
    assert_eq!(facts.nodes.len(), expected.len() + 1, "{:?}", names(&facts));
    for (node, expect) in facts.nodes.iter().skip(1).zip(expected) {
        assert_eq!(node.fact.name, expect.name, "name");
        assert_eq!(node.fact.kind.as_str(), "symbol", "{}", expect.name);
        assert_eq!(node.fact.symbol_kind.map_or("", dctx_map_model::SymbolKind::as_str), expect.symbol, "{}", expect.name);
        assert_eq!(node.fact.public, expect.public, "{}", expect.name);
        assert_eq!(node.precision, Precision::Structural, "{}", expect.name);
        assert_eq!(node.fact.signature, expect.signature, "{}", expect.name);
        if let Some(text) = expect.doc {
            assert_eq!(node.fact.summary.source.as_str(), "doc", "{}", expect.name);
            assert_eq!(node.fact.summary.text, text, "{}", expect.name);
        } else {
            assert_eq!(node.fact.summary.source.as_str(), "sig", "{}", expect.name);
            assert_eq!(node.fact.summary.text, expect.signature, "{}", expect.name);
        }
    }
    assert_eq!(id_of(&facts, "App.Sample::Box::Run"), "s:src/App.cs#App.Sample::Box::Run");
    assert_parent(&facts, "App.Sample", "src/App.cs");
    assert_parent(&facts, "App.Sample::Box", "App.Sample");
    assert_parent(&facts, "App.Sample::Box::capacity", "App.Sample::Box");
    assert_parent(&facts, "App.Sample::Box::Nested::Ping", "App.Sample::Box::Nested");
    assert_parent(&facts, "App.Sample::Point::X", "App.Sample::Point");
    assert_parent(&facts, "App.Sample::Color::Red", "App.Sample::Color");
    assert_parent(&facts, "App.Sample::ITask::Run", "App.Sample::ITask");
    assert_parent(&facts, "App.Sample::Holder::Value", "App.Sample::Holder");

    let imports: Vec<_> = facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind.as_str() == "imports")
        .map(|reference| (reference.fact.name.as_str(), reference.fact.alias.as_deref(), reference.fact.scope.as_str()))
        .collect();
    assert_eq!(
        imports,
        [
            ("System.Linq", None, ""),
            ("System.Collections.Generic", None, ""),
            ("System.Math", Some("static"), ""),
            ("System.Collections.Generic.List<int>", Some("Alias"), ""),
        ]
    );
    assert!(facts.references.iter().filter(|r| r.fact.kind.as_str() == "imports").all(|r| r.fact.src.as_str() == "m:src/App.cs"));

    let calls: Vec<_> = facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind.as_str() == "calls")
        .map(|reference| (reference.fact.name.as_str(), reference.fact.scope.as_str()))
        .collect();
    assert_eq!(
        calls,
        [
            ("store.Load", "App.Sample::Box::Box"),
            ("service.Run", "App.Sample::Box::Run"),
            ("names.ForEach", "App.Sample::Box::Run"),
            ("sink.Accept", "App.Sample::Box::Run"),
            ("Abs", "App.Sample::Box::Run"),
            ("new Point", "App.Sample::Box::Run"),
            ("sink.Flush", "App.Sample::Box::Run"),
            ("Trace", "App.Sample::Box::DebugOnly"),
            ("pingBack", "App.Sample::Box::Nested::Ping"),
        ]
    );
    let uses = named(&facts, "uses-type");
    for name in ["Base", "List", "string", "int", "Obsolete"] {
        assert!(uses.contains(&name), "{name} missing from {uses:?}");
    }
    for absent in ["T", "where", "class", "Decoy", "Hidden", "Fake", "Raw"] {
        assert!(!uses.iter().any(|item| item.contains(absent)), "{absent} in {uses:?}");
    }
    assert_eq!(named(&facts, "implements"), ["IDisposable"]);
    for banned in ["Decoy", "Hidden", "Fake", "Raw", "DEBUG", "endif", "region"] {
        assert!(names(&facts).iter().all(|name| !name.contains(banned)), "{banned}");
    }
    invariants(BASIC, &facts);
    assert_eq!(facts, CsLexer::default().extract("src/App.cs", BASIC.as_bytes()).expect("repeat"));
}

#[test]
fn incomplete_file_returns_partial_facts_without_panic() {
    let facts = CsLexer::default().extract("src/incomplete.cs", INCOMPLETE.as_bytes()).expect("partial");
    assert_eq!(facts.completion, Completion::Partial);
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    assert_eq!(facts.nodes[0].fact.signature, "App");
    let reasons: Vec<_> = facts.diagnostics.iter().map(|diagnostic| diagnostic.reason).collect();
    assert_eq!(
        reasons,
        vec![DiagnosticReason::UnterminatedString, DiagnosticReason::UnbalancedBracket]
    );
    let expected = [
        ("App", "", Precision::Partial),
        ("App::Ready", "class", Precision::Structural),
        ("App::Ready::Ok", "method", Precision::Structural),
        ("App::Broken", "class", Precision::Partial),
        ("App::Broken::M", "method", Precision::Partial),
        ("App::Later", "class", Precision::Partial),
        ("App::Later::x", "const", Precision::Partial),
    ];
    assert_eq!(facts.nodes.len(), expected.len() + 1, "{:?}", names(&facts));
    for (node, (name, symbol, precision)) in facts.nodes.iter().skip(1).zip(expected) {
        assert_eq!(node.fact.name, name);
        assert_eq!(node.fact.symbol_kind.map_or("", dctx_map_model::SymbolKind::as_str), symbol, "{name}");
        assert_eq!(node.precision, precision, "{name}");
    }
    let ready = facts.nodes.iter().find(|node| node.fact.name == "App::Ready").expect("ready");
    assert_eq!(ready.fact.summary.text, "Ready is complete.");
    assert_eq!(ready.fact.summary.source.as_str(), "doc");
    let field = facts.nodes.iter().find(|node| node.fact.name == "App::Later::x").expect("x");
    assert_eq!(field.fact.signature, "public int x");
    assert!(field.fact.public);
    invariants(INCOMPLETE, &facts);
}

#[test]
#[allow(clippy::cast_possible_truncation, reason = "The fuzz length deliberately uses the PRNG's native low word before applying the 301-byte bound.")]
fn truncated_prefixes_and_arbitrary_bytes_do_not_panic() {
    prefixes("src/App.cs", BASIC);
    prefixes("src/incomplete.cs", INCOMPLETE);
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..40 {
        let len = (xorshift(&mut state) as usize) % 301;
        let mut buf = vec![0_u8; len];
        for byte in &mut buf {
            *byte = xorshift(&mut state).to_le_bytes()[0];
        }
        accept("fuzz.cs", &buf);
    }
    accept("fuzz.cs", b"class A { void M() { a.b(); } }");
    accept("fuzz.cs", b"\"\"\"\n");
    accept("fuzz.cs", b"/*");
    accept("fuzz.cs", b"class A<B<C>> {}");
    accept("fuzz.cs", &vec![b'{'; 300]);
    accept("fuzz.cs", &[b'('; 100]);
    accept("fuzz.cs", &[0xff, 0xfe, b'c', b'l', b'a', b's', b's']);
}

fn prefixes(path: &str, source: &str) {
    let bytes = source.as_bytes();
    for len in 0..=bytes.len() {
        let slice = &bytes[..len];
        let once = CsLexer::default().extract(path, slice);
        let twice = CsLexer::default().extract(path, slice);
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
                    LexError::TooDeep { .. } | LexError::TooManyFacts { .. } | LexError::TooLarge { .. } => {}
                    other => panic!("unexpected refusal {other}"),
                }
            }
            _ => panic!("nondeterministic at {len}"),
        }
    }
}

fn accept(path: &str, bytes: &[u8]) {
    let once = CsLexer::default().extract(path, bytes);
    let twice = CsLexer::default().extract(path, bytes);
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
    let lines = u32::try_from(source.lines().count().max(1)).expect("bounded test coordinate");
    let newlines = u32::try_from(source.bytes().filter(|byte| *byte == b'\n').count()).expect("bounded test coordinate") + 1;
    assert_ne!(facts.nodes, []);
    assert_eq!(facts.nodes[0].fact.kind.as_str(), "module");
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    if facts.diagnostics.is_empty() {
        assert_eq!(facts.completion, Completion::Complete);
    } else {
        assert_eq!(facts.completion, Completion::Partial);
    }
    let mut ids = BTreeSet::new();
    for node in &facts.nodes {
        assert_eq!(node.fact.path, facts.path, "{}", node.fact.name);
        let raw = node.fact.id.as_str();
        let identity = raw.split_once(':').map(|(_, rest)| match rest.split_once('#') {
            Some((path, _)) => path,
            None => rest,
        });
        assert_eq!(identity, Some(facts.path.as_str()), "{raw}");
        assert!(raw.starts_with(node.fact.kind.letter()), "{raw}");
        assert_ne!(node.fact.kind.as_str(), "place", "{}", node.fact.name);
        assert_ne!(node.fact.kind.as_str(), "package", "{}", node.fact.name);
        assert!(node.fact.lines[0] >= 1 && node.fact.lines[1] >= node.fact.lines[0], "{}", node.fact.name);
        assert!(node.fact.lines[1] <= lines && node.fact.lines[1] <= newlines, "{}", node.fact.name);
        assert_eq!(node.fact.fingerprint.len(), 64, "{}", node.fact.name);
        assert!(node.fact.fingerprint.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert!(!node.fact.summary.text.is_empty(), "{}", node.fact.name);
        assert!(!node.fact.summary.text.contains('\n'), "{}", node.fact.name);
        assert!(ids.insert(raw.to_string()), "duplicate id {raw}");
    }
    let mut owned = BTreeSet::from([facts.nodes[0].fact.id.as_str()]);
    loop {
        let before = owned.len();
        for edge in &facts.edges {
            assert_eq!(edge.fact.owner_path, facts.path);
            assert_eq!(edge.fact.kind.as_str(), "contains");
            assert_eq!(edge.fact.conf.as_str(), "exact");
            assert!(edge.fact.kind.permits(edge.fact.conf));
            assert!(ids.contains(edge.fact.src.as_str()), "{}", edge.fact.src);
            assert!(ids.contains(edge.fact.dst.as_str()), "{}", edge.fact.dst);
            if owned.contains(edge.fact.src.as_str()) {
                owned.insert(edge.fact.dst.as_str());
            }
        }
        if owned.len() == before {
            break;
        }
    }
    for node in &facts.nodes {
        assert!(owned.contains(node.fact.id.as_str()), "{} is not owned by the file", node.fact.name);
    }
    for reference in &facts.references {
        assert_ne!(reference.fact.kind.as_str(), "contains");
        assert_ne!(reference.fact.name, "");
        assert_eq!(reference.fact.owner_path, facts.path);
        assert!(ids.contains(reference.fact.src.as_str()), "{}", reference.fact.src);
        assert!(reference.fact.lines[0] >= 1 && reference.fact.lines[1] >= reference.fact.lines[0]);
        assert!(reference.fact.lines[1] <= lines && reference.fact.lines[1] <= newlines);
    }
}

fn names(facts: &LexedFile) -> Vec<&str> {
    facts.nodes.iter().map(|node| node.fact.name.as_str()).collect()
}

fn named<'a>(facts: &'a LexedFile, kind: &str) -> Vec<&'a str> {
    facts
        .references
        .iter()
        .filter(|reference| reference.fact.kind.as_str() == kind)
        .map(|reference| reference.fact.name.as_str())
        .collect()
}

fn id_of<'a>(facts: &'a LexedFile, name: &str) -> &'a str {
    facts.nodes.iter().find(|node| node.fact.name == name).expect(name).fact.id.as_str()
}

fn assert_parent(facts: &LexedFile, child: &str, parent: &str) {
    let child_node = facts.nodes.iter().find(|node| node.fact.name == child).expect(child);
    let edge = facts.edges.iter().find(|edge| edge.fact.dst == child_node.fact.id).expect("edge");
    let src = facts.nodes.iter().find(|node| node.fact.id == edge.fact.src).expect("parent");
    assert_eq!(src.fact.name, parent, "{child}");
}
