//! Public Swift extractor contract, golden corpus and adversarial inputs.
//! Implements PARSERS-v1 refusal and map-store ownership regression checks.
//! Uses only the public extractor and shared model, without IO or dependencies.
use dctx_lex_swift::{
    Completion, DiagnosticReason, LexError, LexedFile, Limits, PARSER_ID, PARSER_REVISION,
    Precision, SwiftLexer, body_fingerprint,
};
use dctx_map_model::{Confidence, EdgeKind, NodeKind, SummarySource, SymbolKind};
use std::collections::BTreeSet;

const BASIC: &str = include_str!("fixtures/basic.swift");
const STRINGS: &str = include_str!("fixtures/strings.swift");
const INCOMPLETE: &str = include_str!("fixtures/incomplete.swift");

fn extract(source: &str) -> LexedFile {
    SwiftLexer::default()
        .extract("Sources/App.swift", source.as_bytes())
        .unwrap()
}

fn invariants(facts: &LexedFile, source: &str) {
    let lines = source.bytes().filter(|&b| b == b'\n').count() as u32 + 1;
    let root = &facts.nodes[0].fact;
    assert_eq!(root.kind, NodeKind::Module);
    assert_eq!(root.path, facts.path);
    let mut ids = BTreeSet::new();
    for node in &facts.nodes {
        let node = &node.fact;
        assert!(ids.insert(node.id.clone()), "duplicate {}", node.id);
        assert_eq!(node.path, facts.path);
        let identity_path = node
            .id
            .as_str()
            .split_once(':')
            .unwrap()
            .1
            .split('#')
            .next()
            .unwrap();
        assert_eq!(identity_path, facts.path);
        assert!(node.id.as_str().starts_with(node.kind.letter()));
        assert!(1 <= node.lines[0] && node.lines[0] <= node.lines[1] && node.lines[1] <= lines);
        assert_eq!(node.fingerprint.len(), 64);
    }
    let mut reached = BTreeSet::from([root.id.clone()]);
    for edge in &facts.edges {
        let edge = &edge.fact;
        assert_eq!(edge.owner_path, facts.path);
        assert!(edge.kind.permits(edge.conf));
        assert_eq!(edge.kind, EdgeKind::Contains);
        assert_eq!(edge.conf, Confidence::Exact);
        assert!(reached.contains(&edge.src));
        assert!(ids.contains(&edge.dst));
        assert!(reached.insert(edge.dst.clone()));
    }
    assert_eq!(reached, ids, "every node must be owned by the file tree");
    for reference in &facts.references {
        let reference = &reference.fact;
        assert!(ids.contains(&reference.src));
        assert_eq!(reference.owner_path, facts.path);
        assert!(
            1 <= reference.lines[0]
                && reference.lines[0] <= reference.lines[1]
                && reference.lines[1] <= lines
        );
    }
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    for diagnostic in &facts.diagnostics {
        assert!(diagnostic.byte as usize <= source.len());
        assert!(source.is_char_boundary(diagnostic.byte as usize));
        assert!(1 <= diagnostic.line && diagnostic.line <= lines);
    }
}

#[test]
fn golden_declarations_signatures_access_and_docs() {
    let facts = extract(BASIC);
    assert_eq!(
        facts.completion,
        Completion::Complete,
        "{:?}",
        facts.diagnostics
    );
    let names: Vec<_> = facts
        .nodes
        .iter()
        .skip(1)
        .map(|n| n.fact.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Box",
            "Box::value",
            "Box::count",
            "Box::init",
            "Box::map",
            "Box::subscript",
            "Choice",
            "Choice::empty",
            "Choice::value",
            "Choice::branch",
            "Service",
            "Service::load",
            "Service::ready",
            "Worker",
            "Worker::run",
            "Parent",
            "Parent::run",
            "extension Box",
            "extension Box::upper",
            "Name",
            "<+>",
            "<+>",
            "debugOnly",
            "releaseOnly",
        ]
    );
    let node = |name: &str| {
        &facts
            .nodes
            .iter()
            .find(|n| n.fact.name == name)
            .unwrap()
            .fact
    };
    assert_eq!(node("Box").summary.text, "Stores a value.");
    assert_eq!(node("Box").summary.source, SummarySource::Doc);
    assert!(node("Box").signature.contains("@main public struct Box<T>"));
    assert!(node("Box").signature.contains("where T: Sendable"));
    assert!(node("Box::map").signature.contains("where U: Sendable"));
    assert!(node("Box::value").public);
    assert!(!node("Box::count").public);
    assert_eq!(node("Choice").summary.text, "Available choices.");
    assert_eq!(node("Worker").symbol_kind, Some(SymbolKind::Class));
    assert_eq!(node("Service").symbol_kind, Some(SymbolKind::Interface));
    let imports: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.fact.kind == EdgeKind::Imports)
        .map(|r| r.fact.name.as_str())
        .collect();
    assert_eq!(imports, ["Foundation", "Other.Value"]);
    for name in [
        "transform",
        "closure",
        "client?.send",
        "value.uppercased",
        "log",
    ] {
        assert!(
            facts
                .references
                .iter()
                .any(|r| r.fact.kind == EdgeKind::Calls && r.fact.name == name),
            "{name}"
        );
    }
    assert!(
        facts
            .references
            .iter()
            .any(|r| r.fact.name == "client.status" && r.precision == Precision::Convention)
    );
    invariants(&facts, BASIC);
}

#[test]
fn golden_strings_and_nested_interpolation() {
    let facts = extract(STRINGS);
    assert_eq!(
        facts.completion,
        Completion::Complete,
        "{:?}",
        facts.diagnostics
    );
    assert_eq!(facts.nodes.len(), 2);
    let calls: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.fact.kind == EdgeKind::Calls)
        .map(|r| r.fact.name.as_str())
        .collect();
    for expected in [
        "outer",
        "inner",
        "format",
        "nested",
        "client?.render",
        "print",
    ] {
        assert!(calls.contains(&expected), "{expected}: {calls:?}");
    }
    assert!(!calls.contains(&"notInterpolation"));
    invariants(&facts, STRINGS);
}

#[test]
fn malformed_files_keep_facts_and_diagnostics() {
    let facts = extract(INCOMPLETE);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(facts.nodes.iter().any(|n| n.fact.name == "Retained::valid"));
    assert!(facts.references.iter().any(|r| r.fact.name == "known"));
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|d| d.reason == DiagnosticReason::UnterminatedString)
    );
    assert!(
        facts
            .nodes
            .iter()
            .any(|n| n.precision == Precision::Partial)
    );
    invariants(&facts, INCOMPLETE);
    for (source, reason) in [
        ("/* unclosed", DiagnosticReason::UnterminatedComment),
        ("let x = #\"open", DiagnosticReason::UnterminatedString),
        ("func f() {]", DiagnosticReason::UnbalancedBracket),
        ("func", DiagnosticReason::UnsupportedName),
        ("let x =", DiagnosticReason::Unrecognized),
    ] {
        let facts = extract(source);
        assert_eq!(facts.completion, Completion::Partial, "{source}");
        assert!(
            facts.diagnostics.iter().any(|d| d.reason == reason),
            "{source}: {:?}",
            facts.diagnostics
        );
        invariants(&facts, source);
    }
}

#[test]
fn ids_are_unique_stable_and_owned_even_for_overloads() {
    for source in [
        BASIC,
        STRINGS,
        INCOMPLETE,
        "func f() {}\nfunc f(_ n: Int) {}\nstruct S { func f() {} }\n",
    ] {
        let first = extract(source);
        assert_eq!(first, extract(source));
        let shifted = extract(&format!("\n// shifted\n{source}"));
        assert_eq!(
            first.nodes.iter().map(|n| &n.fact.id).collect::<Vec<_>>(),
            shifted.nodes.iter().map(|n| &n.fact.id).collect::<Vec<_>>()
        );
        invariants(&first, source);
    }
    assert_eq!(
        extract("func unique() {}").nodes[1].fact.id.as_str(),
        "s:Sources/App.swift#unique"
    );
}

#[test]
fn typed_refusals_have_positive_controls() {
    let lexer = SwiftLexer::default();
    assert!(matches!(
        lexer.extract("a.py", b""),
        Err(LexError::UnsupportedDialect)
    ));
    assert!(matches!(
        lexer.extract("../a.swift", b""),
        Err(LexError::InvalidId(_))
    ));
    assert!(matches!(
        lexer.extract("a.swift", &[255]),
        Err(LexError::InvalidUtf8(_))
    ));
    assert!(lexer.extract("a.SWIFT", b"").is_ok());
    let small = SwiftLexer::new(Limits {
        max_bytes: 4,
        ..Limits::default()
    });
    assert!(small.extract("a.swift", b" ").is_ok());
    assert!(matches!(
        small.extract("a.swift", b"     "),
        Err(LexError::TooLarge { .. })
    ));
    let shallow = SwiftLexer::new(Limits {
        max_depth: 2,
        ..Limits::default()
    });
    assert!(shallow.extract("a.swift", b"func f() { {} }").is_ok());
    for source in [
        "func f() { {{}} }",
        "/* /* /* */ */ */",
        "let x = \"\\(f(1))\"",
    ] {
        assert!(
            matches!(
                shallow.extract("a.swift", source.as_bytes()),
                Err(LexError::TooDeep { .. })
            ),
            "{source}"
        );
    }
    let few = SwiftLexer::new(Limits {
        max_facts: 2,
        ..Limits::default()
    });
    assert!(few.extract("a.swift", b"").is_ok());
    assert!(matches!(
        few.extract("a.swift", b"func f() {}"),
        Err(LexError::TooManyFacts { .. })
    ));
    let wide = SwiftLexer::new(Limits {
        max_bytes: usize::MAX,
        max_depth: u32::MAX,
        max_facts: usize::MAX,
    });
    assert!(wide.extract("a.swift", b"struct S {}").is_ok());
    assert_eq!(PARSER_ID, "dctx-lex-swift");
    assert_eq!(PARSER_REVISION, 1);
    assert_eq!(
        body_fingerprint("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(body_fingerprint(" a\n b "), body_fingerprint("a b"));
}

#[test]
fn every_fixture_prefix_and_arbitrary_bytes_are_panic_free() {
    let lexer = SwiftLexer::default();
    for source in [
        BASIC,
        STRINGS,
        INCOMPLETE,
        "struct Café { let naïve = \"🌍\" }",
    ] {
        for end in 0..=source.len() {
            match lexer.extract("Sources/App.swift", &source.as_bytes()[..end]) {
                Ok(facts) => invariants(&facts, &source[..end]),
                Err(LexError::InvalidUtf8(_)) => assert!(!source.is_char_boundary(end)),
                Err(error) => panic!("prefix {end}: {error}"),
            }
        }
    }
    let mut state = 0x31415926u32;
    for size in 0..512 {
        let bytes: Vec<_> = (0..size)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        if let Ok(facts) = lexer.extract("Sources/App.swift", &bytes) {
            invariants(&facts, std::str::from_utf8(&bytes).unwrap());
        }
        let ascii: Vec<_> = bytes.iter().map(|b| b & 127).collect();
        if let Ok(facts) = lexer.extract("Sources/App.swift", &ascii) {
            invariants(&facts, std::str::from_utf8(&ascii).unwrap());
        }
    }
}

#[test]
fn macros_are_opaque_and_conditionals_are_not_evaluated() {
    let source = "func f() { #opaque(secret())\n visible() }\n#if FLAG\nstruct A {}\n#else\nstruct B {}\n#endif";
    let facts = extract(source);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(!facts.references.iter().any(|r| r.fact.name == "secret"));
    assert!(facts.references.iter().any(|r| r.fact.name == "visible"));
    assert!(facts.nodes.iter().any(|n| n.fact.name == "A"));
    assert!(facts.nodes.iter().any(|n| n.fact.name == "B"));
    invariants(&facts, source);
}

#[test]
fn access_operators_bindings_and_generic_calls_keep_lexical_evidence() {
    let source = "public extension Value {\nfunc f() { specialize<Array<Int>>() ; if ready { work {} } }\nprivate func hidden() {}\n}\nstruct Pair { let a = 1, b = 2; init?() {} ; deinit {} }\ninfix operator ⊗⊗\nfunc ⊗⊗(a: Int, b: Int) -> Int { a * b }";
    let facts = extract(source);
    assert_eq!(
        facts.completion,
        Completion::Complete,
        "{:?}",
        facts.diagnostics
    );
    let find = |name: &str| {
        &facts
            .nodes
            .iter()
            .find(|n| n.fact.name == name)
            .unwrap()
            .fact
    };
    assert!(find("extension Value::f").public);
    assert!(!find("extension Value::hidden").public);
    assert!(facts.nodes.iter().any(|n| n.fact.name == "Pair::b"));
    assert!(find("Pair::init").signature.contains("init?()"));
    assert!(facts.nodes.iter().any(|n| n.fact.name == "Pair::deinit"));
    assert_eq!(
        facts.nodes.iter().filter(|n| n.fact.name == "⊗⊗").count(),
        2
    );
    assert!(
        facts
            .references
            .iter()
            .any(|r| r.fact.kind == EdgeKind::Calls && r.fact.name == "specialize")
    );
    assert!(!facts.references.iter().any(|r| r.fact.name == "ready"));
    invariants(&facts, source);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPTS: &str = include_str!("fixtures/scripts.swift");

    #[test]
    fn scripts_availability_attributes_and_trailing_closures_parse() {
        let facts = extract(SCRIPTS);
        assert_eq!(
            facts.completion,
            Completion::Complete,
            "{:?}",
            facts.diagnostics
        );
        let names: Vec<_> = facts
            .nodes
            .iter()
            .skip(1)
            .map(|n| n.fact.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Plugin",
                "Plugin::title",
                "Plugin::Rows",
                "Plugin::render",
                "Session",
                "Session::load",
                "kept",
            ]
        );
        let plugin = &facts
            .nodes
            .iter()
            .find(|n| n.fact.name == "Plugin")
            .unwrap()
            .fact;
        assert!(plugin.signature.contains("@objc(Plugin)"));
        assert!(plugin.public);
        let render = &facts
            .nodes
            .iter()
            .find(|n| n.fact.name == "Plugin::render")
            .unwrap()
            .fact;
        assert!(render.signature.contains("async"));
        assert!(render.signature.contains("some Rows"));
        let title = &facts
            .nodes
            .iter()
            .find(|n| n.fact.name == "Plugin::title")
            .unwrap()
            .fact;
        assert!(title.signature.contains("@Wrapper(name: \"title\")"));
        let load = &facts
            .nodes
            .iter()
            .find(|n| n.fact.name == "Session::load")
            .unwrap()
            .fact;
        assert!(load.signature.contains("any Sendable"));
        for call in [
            "optionalName",
            "show",
            "banner",
            "showLegacy",
            "fallback",
            "finish",
            "report",
            "fetch",
            "print",
            "use",
        ] {
            assert!(
                facts
                    .references
                    .iter()
                    .any(|r| r.fact.kind == EdgeKind::Calls && r.fact.name == call),
                "{call}: {:?}",
                facts
                    .references
                    .iter()
                    .map(|r| r.fact.name.as_str())
                    .collect::<Vec<_>>()
            );
        }
        assert!(!facts.references.iter().any(|r| r.fact.name == "items"));
        invariants(&facts, SCRIPTS);
    }
}
