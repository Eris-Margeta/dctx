//! Golden fixtures, store invariants and deterministic fuzz for the Kotlin extractor.
//! Implements PARSERS-v1: partial facts for malformed input, no panic on arbitrary bytes.
//! Every accepted node is owned by the file, with a span inside that file.
#![forbid(unsafe_code)]

use dctx_lex_kt::{
    Completion, DiagnosticReason, Dialect, KotlinLexer, LexError, LexedFile, Precision, PARSER_ID,
    PARSER_REVISION,
};
use std::collections::BTreeSet;

const BASIC: &str = include_str!("fixtures/basic.kt");
const INCOMPLETE: &str = include_str!("fixtures/incomplete.kt");

fn extract(path: &str, source: &str) -> LexedFile {
    KotlinLexer::default()
        .extract(path, source.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn names(facts: &LexedFile) -> Vec<&str> {
    facts.nodes.iter().map(|node| node.fact.name.as_str()).collect()
}

fn labeled<'a>(facts: &'a LexedFile, name: &str) -> &'a dctx_lex_kt::Labeled<dctx_map_model::Node> {
    facts.nodes.iter().find(|node| node.fact.name == name).unwrap_or_else(|| panic!("missing {name}"))
}

fn node<'a>(facts: &'a LexedFile, name: &str) -> &'a dctx_map_model::Node {
    &facts.nodes.iter().find(|node| node.fact.name == name).unwrap_or_else(|| panic!("missing {name}")).fact
}

fn calls(facts: &LexedFile) -> Vec<&str> {
    facts.references.iter().filter(|item| item.fact.kind.as_str() == "calls").map(|item| item.fact.name.as_str()).collect()
}

fn imports(facts: &LexedFile) -> Vec<(&str, Option<&str>)> {
    facts
        .references
        .iter()
        .filter(|item| item.fact.kind.as_str() == "imports")
        .map(|item| (item.fact.name.as_str(), item.fact.alias.as_deref()))
        .collect()
}

fn uses(facts: &LexedFile) -> Vec<&str> {
    facts
        .references
        .iter()
        .filter(|item| item.fact.kind.as_str() == "uses-type")
        .map(|item| item.fact.name.as_str())
        .collect()
}

fn file_lines(source: &str) -> u32 {
    source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1
}

fn check_store(source: &str, facts: &LexedFile) {
    let lines = file_lines(source);
    assert!(!facts.nodes.is_empty(), "{}", facts.path);
    assert_eq!(facts.nodes[0].fact.kind.as_str(), "module");
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    let completion = if facts.diagnostics.is_empty() { Completion::Complete } else { Completion::Partial };
    assert_eq!(facts.completion, completion);
    let mut ids = BTreeSet::new();
    for node in &facts.nodes {
        let fact = &node.fact;
        let identity = fact.id.as_str().split_once(':').map(|(_, rest)| match rest.split_once('#') {
            Some((path, _)) => path,
            None => rest,
        });
        assert_eq!(fact.path, facts.path, "{}", fact.name);
        assert_eq!(identity, Some(facts.path.as_str()), "{}", fact.id);
        assert!(fact.id.as_str().starts_with(fact.kind.letter()), "{}", fact.id);
        assert!(fact.lines[0] >= 1 && fact.lines[1] >= fact.lines[0], "{}", fact.name);
        assert!(fact.lines[1] <= lines, "{} spans past the file", fact.name);
        assert_eq!(fact.fingerprint.len(), 64, "{}", fact.name);
        assert!(ids.insert(fact.id.as_str().to_string()), "duplicate id {}", fact.id);
    }
    let mut owned = BTreeSet::from([facts.nodes[0].fact.id.as_str()]);
    loop {
        let before = owned.len();
        for edge in &facts.edges {
            let fact = &edge.fact;
            assert_eq!(fact.owner_path, facts.path);
            assert!(fact.kind.permits(fact.conf), "{}", fact.kind.as_str());
            assert_eq!(fact.kind.as_str(), "contains");
            assert_eq!(fact.conf.as_str(), "exact");
            assert!(ids.contains(fact.src.as_str()), "{}", fact.src);
            assert!(ids.contains(fact.dst.as_str()), "{}", fact.dst);
            if owned.contains(fact.src.as_str()) {
                owned.insert(fact.dst.as_str());
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
        let fact = &reference.fact;
        assert_eq!(fact.owner_path, facts.path);
        assert!(!fact.name.is_empty());
        assert!(ids.contains(fact.src.as_str()), "{}", fact.src);
        let [start, end] = fact.lines;
        assert!(start >= 1 && end >= start && end <= lines, "{start}..{end}");
    }
}

fn assert_stable(path: &str, bytes: &[u8]) {
    let lexer = KotlinLexer::default();
    let first = lexer.extract(path, bytes);
    let second = lexer.extract(path, bytes);
    match (&first, &second) {
        (Ok(left), Ok(right)) => {
            let source = std::str::from_utf8(bytes).expect("utf-8");
            check_store(source, left);
            assert_eq!(left, right);
        }
        (Err(left), Err(right)) => {
            assert!(matches!(
                left,
                LexError::InvalidUtf8(_) | LexError::TooDeep { .. } | LexError::TooManyFacts { .. }
            ));
            assert_eq!(left, right);
        }
        _ => panic!("non-deterministic outcome for {path}"),
    }
}

fn xorshift(state: &mut u64) -> u64 {
    let mut value = *state;
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *state = value;
    value
}

#[test]
fn basic_fixture_matches_the_golden_shape() {
    let facts = extract("src/App.kt", BASIC);
    check_store(BASIC, &facts);
    assert_eq!(facts.dialect, Dialect::Kotlin);
    assert!(facts.nodes.iter().all(|node| node.precision == Precision::Structural));
    assert!(facts.edges.iter().all(|edge| edge.precision == Precision::Structural));
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    assert_eq!(
        names(&facts),
        [
            "src/App.kt", "Box", "Box::Box", "Box::item", "Box::size", "Box::show", "Point",
            "Point::Point", "Point::x", "Point::y", "Color", "Color::RED", "Color::GREEN",
            "Color::GREEN::code", "Color::n", "Task", "Task::run", "Registry", "Registry::add",
            "Id", "identity", "trimmed", "App", "App::greeting", "App::start",
        ]
    );
    assert_eq!(facts.nodes[0].fact.signature, "com.example.app");
    assert_eq!(facts.nodes[0].fact.summary.text, "Sample types.");
    assert_eq!(facts.nodes[0].fact.summary.source.as_str(), "doc");
    assert_eq!(node(&facts, "Box").summary.text, "A box of values.");
    assert_eq!(node(&facts, "Box::size").summary.text, "Size of the box.");
    assert_eq!(node(&facts, "Point").summary.text, "Holds a point.");
    assert_eq!(node(&facts, "Box").signature, "open class Box<T : CharSequence>(public val item: T) : Base()");
    assert_eq!(node(&facts, "Box::Box").signature, "(public val item: T)");
    assert_eq!(node(&facts, "Point::Point").signature, "(val x: Int, val y: Int)");
    assert_eq!(node(&facts, "Box::show").signature, "fun show(): String");
    assert_eq!(node(&facts, "identity").signature, "fun <T> identity(value: T): T");
    assert_eq!(node(&facts, "trimmed").signature, "fun String?.trimmed(): String");
    assert_eq!(node(&facts, "App::greeting").signature, "private val greeting: String");
    assert!(!node(&facts, "App::greeting").public);
    assert!(node(&facts, "Box::item").public);
    assert_eq!(node(&facts, "Box").symbol_kind.map(|kind| kind.as_str()), Some("class"));
    assert_eq!(node(&facts, "Point").symbol_kind.map(|kind| kind.as_str()), Some("struct"));
    assert_eq!(node(&facts, "Color").symbol_kind.map(|kind| kind.as_str()), Some("enum"));
    assert_eq!(node(&facts, "Task").symbol_kind.map(|kind| kind.as_str()), Some("interface"));
    assert_eq!(node(&facts, "Id").symbol_kind.map(|kind| kind.as_str()), Some("type"));
    assert_eq!(node(&facts, "identity").symbol_kind.map(|kind| kind.as_str()), Some("fn"));
    assert_eq!(node(&facts, "Box::show").symbol_kind.map(|kind| kind.as_str()), Some("method"));
    assert_eq!(node(&facts, "Box::item").symbol_kind.map(|kind| kind.as_str()), Some("const"));
    assert_eq!(
        imports(&facts),
        [("kotlin.collections.List", None), ("kotlin.collections.*", None), ("com.example.util.Names", Some("Labels"))]
    );
    assert_eq!(
        calls(&facts),
        ["Base", "render", "store.save", "trim", "hidden", "box.show", "Registry.add", "identity", "items.forEach", "item.go"]
    );
    let types = uses(&facts);
    for name in ["CharSequence", "Base", "String"] {
        assert!(types.contains(&name), "missing uses-type {name} in {types:?}");
    }
    assert!(!names(&facts).iter().any(|name| name.contains("Ghost") || name.contains("NotAClass") || name.contains("Decoy")));
    assert!(!calls(&facts).iter().any(|name| *name == "item.length" || *name == "length"));
    assert_eq!(PARSER_ID, "dctx-lex-kt");
    assert_eq!(PARSER_REVISION, 1);
}

#[test]
fn incomplete_file_keeps_later_declarations() {
    let facts = extract("src/Partial.kt", INCOMPLETE);
    check_store(INCOMPLETE, &facts);
    assert_eq!(facts.completion, Completion::Partial);
    assert_eq!(facts.nodes[0].fact.signature, "p");
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnterminatedString));
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnbalancedBracket));
    assert_eq!(names(&facts), ["src/Partial.kt", "Ready", "Ready::ok", "Broken", "Later", "tail"]);
    assert_eq!(node(&facts, "Ready").summary.text, "Ready is complete.");
    assert_eq!(labeled(&facts, "Ready").precision, Precision::Structural);
    assert_eq!(facts.nodes.iter().find(|item| item.fact.name == "Ready::ok").expect("ok").precision, Precision::Structural);
    for name in ["Broken", "Later", "tail"] {
        let labeled = facts.nodes.iter().find(|item| item.fact.name == name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(labeled.precision, Precision::Partial, "{name}");
    }
}

#[test]
fn duplicate_functions_get_stable_ordinals() {
    let facts = extract("dup.kt", "fun f() {}\nfun f() {}\n");
    check_store("fun f() {}\nfun f() {}\n", &facts);
    assert_eq!(facts.nodes[1].fact.name, "f");
    assert_eq!(facts.nodes[2].fact.name, "f");
    assert_eq!(facts.nodes[1].fact.id.as_str(), "s:dup.kt#f~1");
    assert_eq!(facts.nodes[2].fact.id.as_str(), "s:dup.kt#f~2");
}

#[test]
fn tilde_in_a_name_is_abstained() {
    let source = "fun `a~b`() {}\n";
    let facts = extract("n.kt", source);
    check_store(source, &facts);
    assert_eq!(names(&facts), ["n.kt"]);
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnsupportedName));
}

#[test]
fn arbitrary_bytes_and_truncated_fixtures_stay_bounded() {
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..200 {
        let len = (xorshift(&mut state) as usize) % 301;
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            bytes.push(xorshift(&mut state) as u8);
        }
        assert_stable("f.kt", &bytes);
    }
    for source in [BASIC, INCOMPLETE] {
        for len in 0..=source.len() {
            let prefix = &source.as_bytes()[..len];
            let facts = KotlinLexer::default().extract("f.kt", prefix).unwrap_or_else(|error| panic!("prefix {len}: {error}"));
            check_store(&source[..len], &facts);
            assert_stable("f.kt", prefix);
        }
    }
}
