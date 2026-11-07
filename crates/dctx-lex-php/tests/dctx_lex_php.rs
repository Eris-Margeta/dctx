// SPDX-License-Identifier: Apache-2.0
//! Golden PHP facts, typed refusals and deterministic adversarial input checks.
//! Implements decision 077 and DX-PW store ownership, identity and span invariants.
//! Tests the public API only; no external parser, store or fuzz dependency.
use dctx_lex_php::{
    Completion, DiagnosticReason, Dialect, LexError, LexedFile, Limits, PARSER_ID, PARSER_REVISION,
    PhpLexer, Precision, body_fingerprint,
};
use dctx_map_model::{EdgeKind, NodeKind, SummarySource, SymbolKind};
use std::collections::BTreeSet;

const BASIC: &[u8] = include_bytes!("fixtures/basic.php");
const TEMPLATE: &[u8] = include_bytes!("fixtures/template.phtml");
const INCOMPLETE: &[u8] = include_bytes!("fixtures/incomplete.php");

fn parse(path: &str, bytes: &[u8]) -> LexedFile {
    PhpLexer::default().extract(path, bytes).unwrap()
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "This invariant helper only receives the fixed fixtures, their prefixes and fuzz inputs of at most 517 bytes."
)]
fn invariant(path: &str, bytes: &[u8], file: &LexedFile) {
    let source = std::str::from_utf8(bytes).unwrap();
    let lines = source.lines().count().max(1) as u32;
    assert_eq!(file.path, path);
    assert_eq!(file.nodes[0].fact.kind, NodeKind::Module);
    assert_eq!(file.nodes[0].fact.lines, [1, lines]);
    let mut ids = BTreeSet::new();
    for labeled in &file.nodes {
        let node = &labeled.fact;
        assert_eq!(node.path, path);
        let rest = node.id.as_str().split_once(':').unwrap().1;
        assert_eq!(rest.split('#').next(), Some(path));
        assert!(node.id.as_str().starts_with(node.kind.letter()));
        assert!(ids.insert(node.id.clone()), "duplicate {}", node.id);
        assert!(node.lines[0] > 0 && node.lines[0] <= node.lines[1] && node.lines[1] <= lines);
        assert_eq!(node.fingerprint.len(), 64);
    }
    let mut owned = BTreeSet::from([file.nodes[0].fact.id.clone()]);
    for edge in &file.edges {
        assert_eq!(edge.fact.owner_path, path);
        assert!(edge.fact.kind.permits(edge.fact.conf));
        assert_eq!(edge.fact.kind, EdgeKind::Contains);
        assert!(ids.contains(&edge.fact.src) && ids.contains(&edge.fact.dst));
        assert!(owned.contains(&edge.fact.src));
        assert!(owned.insert(edge.fact.dst.clone()));
    }
    assert_eq!(ids, owned, "every node must be owned by the file node");
    for reference in &file.references {
        assert!(ids.contains(&reference.fact.src));
        assert_eq!(reference.fact.owner_path, path);
        assert!(reference.fact.lines[0] > 0);
        assert!(
            reference.fact.lines[0] <= reference.fact.lines[1] && reference.fact.lines[1] <= lines
        );
        assert_ne!(reference.fact.kind, EdgeKind::Contains);
    }
    assert!(file.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    for diagnostic in &file.diagnostics {
        assert!(diagnostic.byte as usize <= bytes.len());
        assert!(diagnostic.line > 0 && diagnostic.line <= lines);
    }
}

#[test]
fn golden_declarations_signatures_visibility_and_phpdoc() {
    let file = parse("src/basic.php", BASIC);
    assert_eq!(
        file.completion,
        Completion::Complete,
        "{:?}",
        file.diagnostics
    );
    let names: Vec<_> = file
        .nodes
        .iter()
        .skip(1)
        .map(|n| n.fact.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "App\\Domain",
            "App\\Domain::Logging",
            "App\\Domain::Logging::log",
            "App\\Domain::Contract",
            "App\\Domain::Contract::count",
            "App\\Domain::Service",
            "App\\Domain::Service::KIND",
            "App\\Domain::Service::OTHER",
            "App\\Domain::Service::$port",
            "App\\Domain::Service::$count",
            "App\\Domain::Service::$total",
            "App\\Domain::Service::run",
            "App\\Domain::Service::count",
            "App\\Domain::Status",
            "App\\Domain::Status::Ready",
            "App\\Domain::Status::count",
            "App\\Domain::create",
            "App\\Domain::GLOBAL_FLAG"
        ]
    );
    let service = file
        .nodes
        .iter()
        .find(|n| n.fact.name == "App\\Domain::Service")
        .unwrap();
    assert_eq!(service.fact.symbol_kind, Some(SymbolKind::Class));
    assert_eq!(service.fact.summary.source, SummarySource::Doc);
    assert_eq!(service.fact.summary.text, "Handles requests.");
    assert!(
        service
            .fact
            .signature
            .contains("extends Base implements Contract")
    );
    let port = file
        .nodes
        .iter()
        .find(|n| n.fact.name.ends_with("::$port"))
        .unwrap();
    assert!(!port.fact.public);
    assert_eq!(port.fact.symbol_kind, None);
    assert_eq!(port.fact.signature, "private readonly ?Port $port");
    let run = file
        .nodes
        .iter()
        .find(|n| n.fact.name.ends_with("::run"))
        .unwrap();
    assert!(run.fact.public);
    assert_eq!(run.fact.symbol_kind, Some(SymbolKind::Method));
    assert_eq!(run.fact.summary.text, "Runs a request.");
    assert_eq!(
        run.fact.signature,
        "public function run(Port $port, array $options = []): \\Vendor\\Result"
    );
    assert!(
        file.nodes
            .iter()
            .all(|n| n.precision == Precision::Structural)
    );
    invariant("src/basic.php", BASIC, &file);
}

#[test]
fn golden_imports_inheritance_calls_and_abstentions() {
    let file = parse("basic.php", BASIC);
    let imports: Vec<_> = file
        .references
        .iter()
        .filter(|r| r.fact.kind == EdgeKind::Imports)
        .map(|r| (r.fact.name.as_str(), r.fact.alias.as_deref()))
        .collect();
    assert_eq!(
        imports,
        [
            ("Vendor\\Support\\Base", None),
            ("Vendor\\Support\\Contract", Some("Port")),
            ("Vendor\\Tools\\format", None),
            ("Vendor\\Tools\\render", Some("view")),
            ("Vendor\\Config\\VERSION", None),
            ("bootstrap.php", None),
            ("view.phtml", None)
        ]
    );
    let calls: Vec<_> = file
        .references
        .iter()
        .filter(|r| r.fact.kind == EdgeKind::Calls)
        .map(|r| r.fact.name.as_str())
        .collect();
    assert_eq!(
        calls,
        [
            "record",
            "$this->log",
            "Helper::build",
            "\\Vendor\\Tools\\format"
        ]
    );
    assert!(
        file.references
            .iter()
            .any(|r| r.fact.kind == EdgeKind::Implements && r.fact.name == "\\JsonSerializable")
    );
    assert!(
        file.references
            .iter()
            .any(|r| r.fact.kind == EdgeKind::UsesType && r.fact.name == "Logging")
    );
    assert!(
        file.references
            .iter()
            .any(|r| r.fact.name == "Service::KIND")
    );
    assert!(
        file.references
            .iter()
            .any(|r| r.fact.name == "$this->count" && r.precision == Precision::Convention)
    );
    assert!(!file.references.iter().any(|r| {
        ["eval", "invented_call", "unknown", "$callback", "$dynamic"]
            .contains(&r.fact.name.as_str())
    }));
}

#[test]
fn golden_templates_and_literal_boundaries() {
    let file = parse("template.phtml", TEMPLATE);
    assert_eq!(
        file.completion,
        Completion::Complete,
        "{:?}",
        file.diagnostics
    );
    let names: Vec<_> = file
        .nodes
        .iter()
        .skip(1)
        .map(|n| n.fact.name.as_str())
        .collect();
    assert_eq!(names, ["template", "after_template"]);
    let calls: Vec<_> = file
        .references
        .iter()
        .filter(|r| r.fact.kind == EdgeKind::Calls)
        .map(|r| r.fact.name.as_str())
        .collect();
    assert_eq!(calls, ["render", "template", "real_call"]);
    invariant("template.phtml", TEMPLATE, &file);
    assert_eq!(
        parse("opaque.inc", b"class Html { bogus(); }").nodes.len(),
        1
    );
}

#[test]
fn malformed_files_retain_partial_facts() {
    let file = parse("incomplete.php", INCOMPLETE);
    assert_eq!(file.completion, Completion::Partial);
    assert!(
        file.diagnostics
            .iter()
            .any(|d| d.reason == DiagnosticReason::UnterminatedString)
    );
    assert!(
        file.diagnostics
            .iter()
            .any(|d| d.reason == DiagnosticReason::UnbalancedBracket)
    );
    assert!(
        file.nodes
            .iter()
            .any(|n| n.fact.name == "Broken::complete" && n.precision == Precision::Structural)
    );
    assert!(
        file.nodes
            .iter()
            .any(|n| n.fact.name == "Broken::Half::start" && n.precision == Precision::Partial)
    );
    assert!(file.references.iter().any(|r| r.fact.name == "known"));
    invariant("incomplete.php", INCOMPLETE, &file);
    for source in [
        "<?php /*",
        "<?php $s = <<<'END'\nbody",
        "<?php function ;",
        "<?php use Foo as ;",
        "<?php ]",
    ] {
        assert_eq!(
            parse("bad.php", source.as_bytes()).completion,
            Completion::Partial,
            "{source}"
        );
    }
}

#[test]
fn store_invariants_and_stable_ids_cover_every_truncated_fixture_prefix() {
    for (path, bytes) in [
        ("basic.php", BASIC),
        ("template.phtml", TEMPLATE),
        ("incomplete.php", INCOMPLETE),
    ] {
        for end in 0..=bytes.len() {
            let prefix = &bytes[..end];
            let file = parse(path, prefix);
            invariant(path, prefix, &file);
            assert_eq!(file, parse(path, prefix), "unstable facts at {path}:{end}");
        }
    }
    let duplicates = b"<?php function same() {} function same() {} class A { function same() {} }";
    let first = parse("stable.php", duplicates);
    invariant("stable.php", duplicates, &first);
    let shifted = [b"\n\n".as_slice(), duplicates].concat();
    let shifted = parse("stable.php", &shifted);
    assert_eq!(
        first.nodes.iter().map(|n| &n.fact.id).collect::<Vec<_>>(),
        shifted.nodes.iter().map(|n| &n.fact.id).collect::<Vec<_>>()
    );
}

#[test]
fn deterministic_fuzz_covers_arbitrary_bytes_and_php_token_noise() {
    let mut state = 0x1234_5678_u64;
    for length in 0..512 {
        let mut bytes = Vec::new();
        for _ in 0..length {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            bytes.push(state.to_le_bytes()[0]);
        }
        match PhpLexer::default().extract("fuzz.inc", &bytes) {
            Ok(file) => invariant("fuzz.inc", &bytes, &file),
            Err(error) => assert!(matches!(error, LexError::InvalidUtf8(_))),
        }
        let alphabet = b"abc_$(){}[];\\'\"#/?<>:=, \n";
        let mut php = b"<?php ".to_vec();
        php.extend(bytes.iter().map(|b| alphabet[*b as usize % alphabet.len()]));
        match PhpLexer::default().extract("fuzz.php", &php) {
            Ok(file) => invariant("fuzz.php", &php, &file),
            Err(error) => assert!(matches!(error, LexError::TooDeep { .. })),
        }
    }
}

#[test]
fn typed_refusals_have_positive_controls() {
    let lexer = PhpLexer::default();
    assert!(matches!(
        lexer.extract("bad.py", b""),
        Err(LexError::UnsupportedDialect)
    ));
    assert!(matches!(
        lexer.extract("../bad.php", b""),
        Err(LexError::InvalidId(_))
    ));
    assert!(matches!(
        lexer.extract("bad.php", &[255]),
        Err(LexError::InvalidUtf8(_))
    ));
    for path in ["ok.php", "ok.PHTML", "ok.inc"] {
        assert_eq!(parse(path, b"").dialect, Dialect::Php);
    }
    let small = PhpLexer::new(Limits {
        max_bytes: 6,
        ..Limits::default()
    });
    assert!(small.extract("ok.php", b"<?php ").is_ok());
    assert!(matches!(
        small.extract("ok.php", b"<?php x"),
        Err(LexError::TooLarge { .. })
    ));
    let shallow = PhpLexer::new(Limits {
        max_depth: 2,
        ..Limits::default()
    });
    assert!(shallow.extract("ok.php", b"<?php {{}}").is_ok());
    assert!(matches!(
        shallow.extract("ok.php", b"<?php {{{}}}"),
        Err(LexError::TooDeep { .. })
    ));
    let sparse = PhpLexer::new(Limits {
        max_facts: 1,
        ..Limits::default()
    });
    assert!(sparse.extract("ok.php", b"<?php ").is_ok());
    assert!(matches!(
        sparse.extract("ok.php", b"<?php f();"),
        Err(LexError::TooManyFacts { .. })
    ));
    assert!(matches!(
        sparse.extract("ok.php", b"<?php function f() {}"),
        Err(LexError::TooManyFacts { .. })
    ));
    assert_eq!(PARSER_ID, "dctx-lex-php");
    assert_eq!(PARSER_REVISION, 1);
    assert_eq!(body_fingerprint(" a\n b "), body_fingerprint("a b"));
}

#[test]
fn namespace_blocks_promoted_properties_attributes_and_static_members() {
    let source = br"<?php
namespace One {
    use Lib\{function f as run, const FLAG, Thing};
    #[First, \Second(Thing::class)]
    class Box {
        public static int $size = 0;
        public function __construct(private readonly Thing $thing, public int $n = 0) {}
        public function &value(): Thing { Box::$size; return $this->thing; }
    }
    function call(): void { run(); Box::value(...); $$this->invented(); }
}
namespace { function root(): void { \One\call(); } }
";
    let file = parse("blocks.inc", source);
    assert_eq!(
        file.completion,
        Completion::Complete,
        "{:?}",
        file.diagnostics
    );
    invariant("blocks.inc", source, &file);
    for name in [
        "One::Box::$thing",
        "One::Box::$n",
        "One::Box::value",
        "root",
    ] {
        assert!(file.nodes.iter().any(|n| n.fact.name == name), "{name}");
    }
    for name in [
        "Lib\\f",
        "Lib\\FLAG",
        "Lib\\Thing",
        "First",
        "\\Second",
        "Box::$size",
        "\\One\\call",
    ] {
        assert!(
            file.references.iter().any(|r| r.fact.name == name),
            "{name}"
        );
    }
    assert!(
        !file
            .references
            .iter()
            .any(|r| r.fact.kind == EdgeKind::Calls
                && ["Box::value", "$this->invented"].contains(&r.fact.name.as_str()))
    );
}
