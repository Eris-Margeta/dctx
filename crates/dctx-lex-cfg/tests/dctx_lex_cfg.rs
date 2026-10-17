//! Golden fixtures, store invariants and deterministic fuzz for configuration files.
//! Implements PARSERS-v1: partial facts for malformed input, no panic on arbitrary bytes.
//! Store rules match dctx-map-store: file ownership, unique ids, spans inside the file.
#![forbid(unsafe_code)]

use dctx_lex_cfg::{
    body_fingerprint, Completion, CfgLexer, DiagnosticReason, Dialect, LexedFile, Precision,
    PARSER_ID, PARSER_REVISION,
};
use dctx_map_model::{EdgeKind, NodeKind, SummarySource, SymbolKind};
use std::collections::BTreeSet;

const JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/basic.json"));
const TOML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/basic.toml"));
const YAML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/basic.yaml"));
const PARTIAL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/incomplete.yaml"));

fn extract(path: &str, source: &str) -> LexedFile {
    CfgLexer::default()
        .extract(path, source.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn names(facts: &LexedFile) -> Vec<&str> {
    facts.nodes.iter().map(|node| node.fact.name.as_str()).collect()
}

fn node<'a>(facts: &'a LexedFile, name: &str) -> &'a dctx_map_model::Node {
    &facts
        .nodes
        .iter()
        .find(|node| node.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name} in {:?}", names(facts)))
        .fact
}

fn refs(facts: &LexedFile) -> Vec<&str> {
    facts.references.iter().map(|reference| reference.fact.name.as_str()).collect()
}

fn assert_store(facts: &LexedFile, source: &str) {
    let file_end = source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    assert!(!facts.nodes.is_empty());
    assert_eq!(facts.nodes[0].fact.kind, NodeKind::Module, "{}", facts.path);
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    let expect = if facts.diagnostics.is_empty() { Completion::Complete } else { Completion::Partial };
    assert_eq!(facts.completion, expect);
    let mut ids = BTreeSet::new();
    for labeled in &facts.nodes {
        let node = &labeled.fact;
        assert!(!matches!(node.kind, NodeKind::Place | NodeKind::Package), "{}", node.name);
        assert_eq!(node.path, facts.path, "{}", node.name);
        let identity = node.id.as_str().split_once(':').map(|(_, rest)| match rest.split_once('#') {
            Some((path, _)) => path,
            None => rest,
        });
        assert_eq!(identity, Some(facts.path.as_str()), "{}", node.id);
        assert!(node.id.as_str().starts_with(node.kind.letter()), "{}", node.id);
        assert!(node.lines[0] >= 1 && node.lines[1] >= node.lines[0] && node.lines[1] <= file_end, "{}", node.name);
        assert_eq!(node.fingerprint.len(), 64, "{}", node.name);
        assert!(!node.summary.text.contains(['\n', '\r']));
        assert!(node.summary.text.chars().count() <= 100, "{}", node.name);
        assert!(!node.signature.contains(['\n', '\r']));
        assert!(node.signature.chars().count() <= 160, "{}", node.name);
        assert!(!node.public);
        assert!(ids.insert(node.id.as_str().to_owned()), "duplicate {}", node.id);
    }
    let mut owned = BTreeSet::from([facts.nodes[0].fact.id.as_str()]);
    loop {
        let before = owned.len();
        for edge in &facts.edges {
            assert_eq!(edge.fact.owner_path, facts.path);
            assert!(edge.fact.kind.permits(edge.fact.conf));
            assert!(ids.contains(edge.fact.src.as_str()), "{}", edge.fact.src);
            assert!(ids.contains(edge.fact.dst.as_str()), "{}", edge.fact.dst);
            if edge.fact.kind == EdgeKind::Contains && owned.contains(edge.fact.src.as_str()) {
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
        assert_eq!(reference.fact.kind, EdgeKind::Imports);
        assert!(reference.fact.alias.is_none());
        assert_eq!(reference.fact.owner_path, facts.path);
        assert!(ids.contains(reference.fact.src.as_str()), "{}", reference.fact.name);
        assert!(!reference.fact.name.is_empty());
        let [start, end] = reference.fact.lines;
        assert!(start >= 1 && end >= start && end <= file_end, "{}", reference.fact.name);
    }
}

fn load(path: &str, source: &str) -> LexedFile {
    let facts = extract(path, source);
    assert_store(&facts, source);
    assert_eq!(facts, extract(path, source));
    facts
}

#[test]
fn parser_identity_and_json_golden() {
    assert_eq!(PARSER_ID, "dctx-lex-cfg");
    assert_eq!(PARSER_REVISION, 1);
    let facts = load("basic.json", JSON);
    assert_eq!(facts.dialect, Dialect::Json);
    assert_eq!(facts.completion, Completion::Complete);
    assert!(facts.diagnostics.is_empty());
    assert!(facts.nodes.iter().all(|node| node.precision == Precision::Structural));
    assert!(facts.edges.iter().all(|edge| edge.precision == Precision::Structural));
    assert!(facts.references.iter().all(|reference| reference.precision == Precision::Convention));
    assert_eq!(
        names(&facts),
        [
            "basic.json",
            "package",
            "package.name",
            "package.version",
            "dependencies",
            "dependencies.serde",
            "dependencies.local",
            "dependencies.local.path",
            "workspaces",
            "workspaces.0",
            "workspaces.1",
        ]
    );
    assert_eq!(refs(&facts), ["serde", "local", "../local", "crates/core", "crates/cli"]);
    assert_eq!(facts.nodes[0].fact.id.as_str(), "m:basic.json");
    assert_eq!(facts.nodes[0].fact.fingerprint, body_fingerprint(JSON));
    assert_eq!(node(&facts, "package").symbol_kind, Some(SymbolKind::Struct));
    assert_eq!(node(&facts, "package.name").symbol_kind, None);
    assert_eq!(node(&facts, "package.name").signature, "demo");
    assert_eq!(node(&facts, "package.version").signature, "0.1.0");
    assert_eq!(node(&facts, "dependencies.local.path").signature, "../local");
    assert_eq!(node(&facts, "workspaces.0").signature, "crates/core");
    assert_eq!(node(&facts, "package.name").id.as_str(), "s:basic.json#package.name");
}

#[test]
fn toml_golden_comments_tables_and_paths() {
    let facts = load("basic.toml", TOML);
    assert_eq!(facts.dialect, Dialect::Toml);
    assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
    assert_eq!(
        names(&facts),
        [
            "basic.toml",
            "package",
            "package.name",
            "package.version",
            "dependencies",
            "dependencies.serde",
            "dependencies.local",
            "dependencies.local.path",
            "workspace",
            "workspace.members",
            "workspace.members.0",
            "workspace.members.1",
            "bin",
            "bin.0",
            "bin.0.name",
            "bin.0.path",
            "profile",
            "profile.release",
            "profile.release.lto",
        ]
    );
    assert_eq!(refs(&facts), ["serde", "local", "../local", "crates/core", "crates/cli", "src/main.rs"]);
    assert!(facts.references.iter().all(|reference| reference.precision == Precision::Convention));
    assert_eq!(facts.nodes[0].fact.summary.text, "Package demo is a sample.");
    assert_eq!(facts.nodes[0].fact.summary.source, SummarySource::Doc);
    assert_eq!(node(&facts, "package").summary.text, "Package demo is a sample.");
    let version = node(&facts, "package.version");
    assert_eq!(version.summary.text, "semver");
    assert_eq!(version.summary.source, SummarySource::Doc);
    assert_eq!(version.signature, "0.1.0");
    assert_eq!(node(&facts, "profile.release.lto").signature, "true");
    assert_eq!(node(&facts, "bin").symbol_kind, Some(SymbolKind::Struct));
    assert_eq!(facts.nodes[0].fact.fingerprint, body_fingerprint(TOML));
}

#[test]
fn yaml_golden_anchors_are_not_expanded() {
    let facts = load("basic.yaml", YAML);
    assert_eq!(facts.dialect, Dialect::Yaml);
    assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
    assert_eq!(
        names(&facts),
        [
            "basic.yaml",
            "package",
            "package.name",
            "package.version",
            "dependencies",
            "dependencies.serde",
            "dependencies.local",
            "dependencies.local.path",
            "anchor",
            "anchor.enabled",
            "use",
            "workspace",
            "workspace.members",
            "workspace.members.0",
            "workspace.members.1",
        ]
    );
    assert_eq!(refs(&facts), ["serde", "local", "../local", "crates/core", "crates/cli"]);
    assert_eq!(facts.nodes[0].fact.summary.text, "Service demo.");
    assert_eq!(node(&facts, "package").summary.text, "Service demo.");
    let version = node(&facts, "package.version");
    assert_eq!(version.summary.text, "semver");
    assert_eq!(version.summary.source, SummarySource::Doc);
    assert_eq!(version.signature, "0.1.0");
    assert!(node(&facts, "anchor").signature.contains("&base"));
    assert_eq!(node(&facts, "use").signature, "*base");
    assert!(facts.nodes.iter().all(|node| node.fact.name != "use.enabled"));
}

#[test]
fn yaml_documents_are_indexed_and_a_single_marker_is_not() {
    let docs = load("a.yaml", "---\na: 1\n---\nb: 2\n");
    assert_eq!(names(&docs), ["a.yaml", "0", "0.a", "1", "1.b"]);
    assert_eq!(docs.completion, Completion::Complete);
    assert!(docs.references.is_empty());
    let one = load("a.yaml", "---\na: 1\n");
    assert_eq!(one.nodes[1].fact.name, "a");
    let anchored = load("a.yaml", "base: &base\n  x: 1\nuse: *base\n");
    assert_eq!(names(&anchored), ["a.yaml", "base", "base.x", "use"]);
    assert_eq!(anchored.nodes[3].fact.signature, "*base");
    assert!(anchored.nodes[1].fact.signature.contains("&base"));
    assert!(anchored.references.is_empty());
}

#[test]
fn urls_globs_and_bare_members_abstain() {
    let yaml = load("a.yaml", "site: https://example.com/a\ninclude: crates/*\nmembers:\n  - crates/core\n");
    assert!(yaml.references.is_empty(), "{:?}", refs(&yaml));
    assert_eq!(yaml.completion, Completion::Complete, "{:?}", yaml.diagnostics);
    let toml = load("a.toml", "site = \"https://example.com/a\"\ninclude = \"crates/*\"\nmembers = [\"crates/core\"]\n");
    assert!(toml.references.is_empty(), "{:?}", refs(&toml));
}

#[test]
fn incomplete_yaml_keeps_partial_facts() {
    let facts = load("incomplete.yaml", PARTIAL);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnterminatedString));
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnbalancedBracket));
    for name in ["ok", "nested", "nested.child"] {
        assert!(names(&facts).contains(&name), "{name} missing in {:?}", names(&facts));
    }
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    let precision = |name: &str| facts.nodes.iter().find(|node| node.fact.name == name).unwrap().precision;
    assert_eq!(precision("ok"), Precision::Structural);
    assert_eq!(precision("nested.child"), Precision::Structural);
}

fn assert_stable(path: &str, bytes: &[u8]) {
    let lexer = CfgLexer::default();
    let first = lexer.extract(path, bytes);
    let second = lexer.extract(path, bytes);
    match (&first, &second) {
        (Ok(left), Ok(right)) => {
            if let Ok(source) = std::str::from_utf8(bytes) {
                assert_store(left, source.strip_prefix('\u{feff}').unwrap_or(source));
            }
            assert_eq!(left, right);
        }
        (Err(left), Err(right)) => assert_eq!(left.to_string(), right.to_string()),
        _ => panic!("non-deterministic outcome for {path}"),
    }
}

fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
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
        for path in ["f.json", "f.jsonc", "f.toml", "f.yaml", "f.yml", "f.cfg"] {
            assert_stable(path, &bytes);
        }
    }
    let fixtures = [("basic.json", JSON), ("basic.toml", TOML), ("basic.yaml", YAML), ("incomplete.yaml", PARTIAL)];
    for (path, source) in fixtures {
        for len in 0..=source.len() {
            let prefix = &source.as_bytes()[..len];
            let facts = CfgLexer::default()
                .extract(path, prefix)
                .unwrap_or_else(|error| panic!("{path} prefix {len}: {error}"));
            assert_store(&facts, &source[..len]);
            assert_stable(path, prefix);
        }
    }
}
