//! Public map verbs over the repository fixture.
//! The strict profile reports structural coverage, not the enhanced `syn` label.
//! Refusals leave the sink empty. A successful call sits beside each refusal.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::path::Path;

use dctx::memory::MemorySource;
use dctx::{
    DctxError, ErrorCode, FreshnessMode, IdentityOutcome, MapRequest, RuleTexts, SelectionPolicy,
    SnapshotRequest, VecSink, Verb,
};

use common::{open_session, session_parts, SharedSource, PING, README};

const MODULE: &str = "m:src/lib.rs";
const PING_ID: &str = "s:src/lib.rs#ping";

#[test]
fn fixture_file_is_the_thirty_byte_golden() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repository/src/lib.rs");
    let bytes = std::fs::read(path).expect("fixture");
    assert_eq!(bytes, PING);
    assert_eq!(bytes.len(), 30);
    assert_eq!(&bytes[13..29], b"pub fn ping() {}");
}

#[test]
fn five_verbs_project_the_golden_symbol() {
    let mut session = open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    assert_eq!(handle.generation(), 1);
    // The env-shaped `secrets.env` is never read, so the fixture costs three reads, not four.
    assert_eq!(session.content_reads(), 3);
    assert_eq!(session.cached_content_reads(), 0);
    assert!(handle.digest().starts_with("v1:sha256:"));

    let mut sink = VecSink::new();
    let map = session.map(&handle, MODULE, &mut sink).expect("map");
    assert_eq!(map.records.len(), 1);
    assert_eq!(map.records[0].id, PING_ID);
    assert_eq!(map.records[0].kind, "fn");
    assert_eq!(map.records[0].path, "src/lib.rs");
    assert_eq!(map.records[0].lines, [2, 2]);
    assert_eq!(map.records[0].summary, "Respond.");
    assert_eq!(map.records[0].summary_source, "doc");
    assert!(map.records[0].source_utf8.is_none());
    assert_eq!(map.precision.coverage, "structural");
    assert_eq!(map.precision.completion, "complete");
    assert_eq!(map.precision.parser, "dctx-structural/2");
    assert_eq!(map.freshness.mode, FreshnessMode::ImmutableSnapshot.as_str());
    assert_eq!(map.freshness.state, "verified");
    assert_eq!(map.freshness.age_s, 0);
    assert!(map.outcome.is_none());
    let text = std::str::from_utf8(sink.as_bytes()).expect("utf8");
    assert!(text.contains("\"place\":\"fixture\""));
    assert!(text.contains("\"coverage\":\"structural\""));
    assert!(text.contains("\"parser\":\"dctx-structural/2\""));
    assert!(!text.contains("\"vcs\""));
    assert!(!text.contains("syntax"));

    let mut sink = VecSink::new();
    let found = session.find(&handle, "ping", &mut sink).expect("find");
    assert_eq!(found.records.len(), 1);
    assert_eq!(found.records[0].id, PING_ID);
    assert_eq!(found.records[0].score, 3000);
    assert!(found.records.iter().all(|record| record.kind != "module"));

    let mut sink = VecSink::new();
    let neighbors = session.neighbors(&handle, MODULE, &mut sink).expect("neighbors");
    assert_eq!(neighbors.records.len(), 1);
    assert_eq!(neighbors.records[0].edge.as_deref(), Some("contains"));
    assert_eq!(neighbors.records[0].direction.as_deref(), Some("out"));
    assert_eq!(neighbors.records[0].target.as_deref(), Some(PING_ID));
    assert_eq!(neighbors.records[0].confidence.as_deref(), Some("exact"));

    let before = session.content_reads();
    let mut sink = VecSink::new();
    let explained = session.explain(&handle, PING_ID, &mut sink).expect("explain");
    assert_eq!(session.content_reads(), before);
    assert_eq!(explained.records[0].signature.as_deref(), Some("pub fn ping()"));
    assert_eq!(explained.records[0].summary, "Respond.");
    assert_eq!(explained.records[0].summary_source, "doc");
    assert!(explained.records[0].source_utf8.is_none());

    let mut sink = VecSink::new();
    let read = session.read(&handle, PING_ID, &mut sink).expect("read");
    assert_eq!(session.content_reads(), before + 1);
    assert_eq!(read.records[0].span, Some([13, 29]));
    assert_eq!(read.records[0].source_utf8.as_deref(), Some("pub fn ping() {}"));
    assert_eq!(session.cached_content_reads(), 0);
}

#[test]
fn policy_deny_reads_nothing_new_and_a_grant_still_explains() {
    let mut session = open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let after = session.content_reads();
    let mut sink = VecSink::new();
    let explained = session.explain(&handle, PING_ID, &mut sink).expect("allowed");
    assert_eq!(explained.records[0].id, PING_ID);
    assert!(!sink.is_empty());
    assert_eq!(session.content_reads(), after);

    session.revise_selection(SelectionPolicy::from_texts(&RuleTexts {
        local_omit: Some("src/lib.rs\n"),
        ..RuleTexts::default()
    }));
    let mut denied = VecSink::new();
    let error = session.explain(&handle, PING_ID, &mut denied).expect_err("denied");
    assert_eq!(error, DctxError::PolicyDenied);
    assert!(denied.is_empty());
    assert_eq!(session.content_reads(), after);
    assert_eq!(session.cached_content_reads(), 0);

    let mut kept = VecSink::new();
    let heading = session.explain(&handle, "d:README.md#Fixture", &mut kept).expect("readme");
    assert_eq!(heading.records[0].path, "README.md");
    assert_eq!(heading.records[0].kind, "heading");
    assert_eq!(session.content_reads(), after);
}

#[test]
fn tiny_budget_writes_nothing_after_a_normal_page() {
    let mut session = open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let mut sink = VecSink::new();
    session.map(&handle, MODULE, &mut sink).expect("page");
    assert!(!sink.is_empty());

    let mut request = MapRequest::new(Verb::Map);
    request.target = MODULE.to_string();
    request.limits.encoded_bytes = Some(1);
    request.limits.estimated_tokens = Some(1);
    let mut tiny = VecSink::new();
    let error = session.query(&handle, &request, &mut tiny).expect_err("budget");
    assert_eq!(error.code(), ErrorCode::BudgetTooSmall);
    assert!(matches!(error, DctxError::BudgetTooSmall { minimum_bytes } if minimum_bytes >= 1));
    assert!(tiny.is_empty());
}

#[test]
fn same_stamp_byte_change_emits_no_source_bytes() {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", PING, 0).expect("lib");
    let shared = SharedSource::new(source);
    let handle_source = shared.handle();
    let mut session = open_session(shared);
    let snap = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let mut sink = VecSink::new();
    let read = session.read(&snap, PING_ID, &mut sink).expect("read");
    assert_eq!(read.records[0].source_utf8.as_deref(), Some("pub fn ping() {}"));
    let after = session.content_reads();

    handle_source.lock().expect("lock").insert_file("src/lib.rs", common::PONG, 0).expect("replace");
    let mut changed = VecSink::new();
    let error = session.read(&snap, PING_ID, &mut changed).expect_err("changed");
    assert_eq!(error, DctxError::SourceChanged);
    assert!(changed.is_empty());
    assert_eq!(session.content_reads(), after + 1);
    assert_eq!(session.cached_content_reads(), 0);
}

#[test]
fn gone_is_ok_and_unknown_transport_writes_nothing() {
    let mut session = open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let mut sink = VecSink::new();
    let gone = session.explain(&handle, "s:src/lib.rs#missing", &mut sink).expect("gone");
    assert!(matches!(gone.outcome, Some(IdentityOutcome::Gone { .. })));
    assert_eq!(gone.next.as_deref(), Some("map"));
    assert_eq!(gone.records, [] as [dctx::AnswerRecord; 0]);
    assert!(!sink.is_empty());

    let mut alias = VecSink::new();
    let at = session.explain(&handle, "@ping", &mut alias).expect("alias");
    assert_eq!(at.outcome.as_ref().map(IdentityOutcome::as_str), Some("gone"));
    assert_eq!(at.next.as_deref(), Some("map"));

    let reads = session.content_reads();
    let mut bad_format = VecSink::new();
    let mut request = MapRequest::new(Verb::Explain);
    request.target = PING_ID.to_string();
    request.output_format = 9;
    let error = session.query(&handle, &request, &mut bad_format).expect_err("format");
    assert_eq!(error, DctxError::UnsupportedVersion { found: 9 });
    assert!(bad_format.is_empty());

    request.output_format = 2;
    request.request_version = 4;
    let mut bad_version = VecSink::new();
    let error = session.query(&handle, &request, &mut bad_version).expect_err("version");
    assert_eq!(error, DctxError::UnsupportedVersion { found: 4 });
    assert!(bad_version.is_empty());

    request.request_version = 1;
    request.git_revision = Some("HEAD".to_string());
    let mut git = VecSink::new();
    let error = session.query(&handle, &request, &mut git).expect_err("git");
    assert!(matches!(error, DctxError::UnsupportedCapability { capability: "git", .. }));
    assert!(git.is_empty());
    assert_eq!(session.content_reads(), reads);

    let mut empty = VecSink::new();
    request.git_revision = None;
    request.target.clear();
    let error = session.query(&handle, &request, &mut empty).expect_err("target");
    assert_eq!(error.code(), ErrorCode::InvalidRequest);
    assert!(empty.is_empty());
}

#[test]
fn gitignore_text_is_not_applied_and_git_metadata_is_not_opened() {
    let mut source = MemorySource::new();
    source.insert_dir(".git", 0).expect("git dir");
    source.insert_file(".git/config", b"[core]\n", 0).expect("config");
    source.insert_file("src/lib.rs", PING, 0).expect("lib");
    source.insert_file("README.md", README, 0).expect("readme");
    source.insert_file(".gitignore", b"# kept\nsecrets.env\n", 0).expect("ignore");
    source.insert_file("secrets.env", b"TOKEN=secret\n", 0).expect("secret");
    let shared = SharedSource::new(source);
    let log = shared.handle();
    let mut session = open_session(shared);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    assert_eq!(session.content_reads(), 3);
    let reads = log.lock().expect("log").reads();
    assert!(!reads.iter().any(|path| path == "secrets.env"), "env-shaped files are never read");
    assert!(reads.iter().any(|path| path == ".gitignore"));
    assert!(!reads.iter().any(|path| path == ".git" || path.starts_with(".git/")));
    let mut sink = VecSink::new();
    let secret = session.explain(&handle, "m:secrets.env", &mut sink).expect("secret");
    assert_eq!(secret.outcome.as_ref().map(IdentityOutcome::as_str), Some("gone"));
    let mut git = VecSink::new();
    let hidden = session.explain(&handle, "m:.git/config", &mut git).expect("gone");
    assert_eq!(hidden.outcome.as_ref().map(IdentityOutcome::as_str), Some("gone"));

    let mut omitted = session_parts(
        {
            let mut source = MemorySource::new();
            source.insert_file("src/lib.rs", PING, 0).expect("lib");
            source.insert_file("README.md", README, 0).expect("readme");
            source.insert_file(".gitignore", b"# kept\nsecrets.env\n", 0).expect("ignore");
            source.insert_file("secrets.env", b"TOKEN=secret\n", 0).expect("secret");
            source
        },
        std::sync::Arc::new(dctx::memory::CancelFlag::new()),
        dctx::HostGrants::PermitAll,
        dctx::StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 },
        SelectionPolicy::from_texts(&RuleTexts {
            local_omit: Some("secrets.env\n"),
            ..RuleTexts::default()
        }),
    );
    let handle = omitted.snapshot(&SnapshotRequest::immutable()).expect("omit");
    assert_eq!(omitted.content_reads(), 3);
    let mut sink = VecSink::new();
    let gone = omitted.explain(&handle, "m:secrets.env", &mut sink).expect("omitted");
    assert_eq!(gone.outcome.as_ref().map(IdentityOutcome::as_str), Some("gone"));
}
