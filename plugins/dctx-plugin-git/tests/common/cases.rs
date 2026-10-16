//! The git-cases.json parity corpus: case setup on the fake broker and the shared expectation check.
//! Implements fixtures/README.md "git-cases.json adds exact byte goldens ... Stages/receipts and spy
//! counters must match" (G-PARITY, G-COVERAGE, G-STAGE). Used by parity (direct) and conformance (wire).

use dctx_plugin_git::app::binding::Binding;
use dctx_plugin_protocol::Json;
use dctx_plugin_protocol::records::invocation::Invocation;

use super::{FakeBroker, GitScript, binding, document, hook, sha256_hex, text};

/// One parity case.
#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    pub raw: Json,
    pub input: Option<Vec<u8>>,
}

/// What an invoke produced, in wire words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    pub status: Option<String>,
    pub reason: Option<String>,
    pub code: Option<String>,
    pub diagnostics: Vec<String>,
    pub receipts: usize,
}

pub fn cases() -> Vec<Case> {
    let corpus = document("fixtures/git-cases.json");
    assert_eq!(text(&corpus, "predicate"), "G-PARITY");
    corpus
        .get("cases")
        .and_then(Json::as_array)
        .expect("cases")
        .iter()
        .map(|raw| Case {
            id: text(raw, "id").to_owned(),
            input: raw.get("input").and_then(|input| input.get("utf8")).and_then(Json::as_str).map(|s| s.as_bytes().to_vec()),
            raw: raw.clone(),
        })
        .collect()
}

fn flag(value: &Json, key: &str, default: bool) -> bool {
    value.get(key).and_then(Json::as_bool).unwrap_or(default)
}

/// Fake broker, binding and invocation for a case.
pub fn setup(case: &Case) -> (FakeBroker, Binding, Invocation) {
    let raw = &case.raw;
    let mut broker = FakeBroker::new(case.input.as_deref().unwrap_or_default());
    broker.readable = text(raw, "selection") == "Include";
    if !flag(raw, "git_available", true) {
        broker.git = GitScript::Missing;
    }
    let exists = case.input.is_some() || flag(raw, "file_exists", false);
    let nested: &[&str] = if text(raw, "nested_rule_selection") == "Exclude" { &["file:nested"] } else { &[] };
    if !nested.is_empty() {
        broker.nested.insert("file:nested".into(), None);
    }
    let invocation = hook(text(raw, "host_version"), flag(raw, "stage", true), flag(raw, "repository", true));
    (broker, binding(exists, nested, true), invocation)
}

fn count(expected: &Json, key: &str) -> Option<u64> {
    expected.get(key).and_then(Json::as_int).and_then(|n| u64::try_from(n).ok())
}

/// Checks status/reason/code, diagnostics, spy counters and exact byte goldens.
pub fn check(case: &Case, observed: &Observed, broker: &FakeBroker) {
    let id = &case.id;
    let expected = case.raw.get("expected").expect("expected");
    if let Some(code) = expected.get("code").and_then(Json::as_str) {
        assert_eq!(observed.code.as_deref(), Some(code), "{id}");
    } else {
        assert_eq!(observed.status.as_deref(), Some(text(expected, "status")), "{id}");
        assert_eq!(observed.reason.as_deref(), Some(text(expected, "reason")), "{id}");
    }
    if let Some(diagnostic) = expected.get("diagnostic").and_then(Json::as_str) {
        assert!(observed.diagnostics.iter().any(|code| code == diagnostic), "{id}: {:?}", observed.diagnostics);
    }
    for (key, actual) in [("writes", broker.writes), ("git_spawns", broker.git_spawns), ("reads", broker.reads), ("hashes", broker.hashes)] {
        if let Some(want) = count(expected, key) {
            assert_eq!(actual, want, "{id}: {key}");
        }
    }
    if let (Some(input), Some(raw_input)) = (&case.input, case.raw.get("input")) {
        assert_eq!(sha256_hex(input), text(raw_input, "sha256"), "{id}: input digest");
        assert_eq!(count(raw_input, "bytes"), Some(input.len() as u64), "{id}: input length");
    }
    if let Some(golden) = expected.get("utf8").and_then(Json::as_str) {
        assert_eq!(String::from_utf8_lossy(&broker.file), golden, "{id}: committed bytes");
        assert_eq!(sha256_hex(&broker.file), text(expected, "sha256"), "{id}: committed digest");
        assert_eq!(count(expected, "bytes"), Some(broker.file.len() as u64), "{id}: committed length");
    }
    if broker.git_spawns > 0 {
        assert_eq!(broker.staged.as_deref(), Some(broker.file.as_slice()), "{id}: whole current file staged");
    }
    assert_eq!(observed.receipts as u64, broker.writes + broker.git_spawns, "{id}: receipts match effects");
}
