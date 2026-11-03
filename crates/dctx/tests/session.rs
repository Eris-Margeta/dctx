// SPDX-License-Identifier: Apache-2.0
//! Cancellation, pack publication, continuation and cache administration.
//! A cancelled snapshot leaves no generation. A pin blocks collection until it is dropped.
//! Purge without confirmation changes nothing.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use dctx::memory::{CancelFlag, MemorySource};
use dctx::{
    DctxError, ErrorCode, GcRequest, MapRequest, OperationLimits, PackRequest, PurgeRequest, PurgeScope,
    Resource, SnapshotRequest, StoragePolicy, VecSink, Verb,
};

use common::{open_session, session_parts, CancelAfter, PING};

#[test]
fn cancel_before_snapshot_and_during_the_walk_keep_no_generation() {
    let flag = Arc::new(CancelFlag::new());
    flag.cancel();
    let mut session = session_parts(
        common::repository(),
        flag,
        dctx::HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 },
        dctx::SelectionPolicy::from_texts(&dctx::RuleTexts::default()),
    );
    let error = session.snapshot(&SnapshotRequest::immutable()).expect_err("cancelled");
    assert_eq!(error, DctxError::Cancelled);
    assert_eq!(session.generation_count(), 0);
    assert_eq!(session.content_reads(), 0);

    let shared = common::SharedSource::new(common::repository());
    let log = shared.handle();
    let mut session = session_parts(
        shared,
        Arc::new(CancelAfter::new(3)),
        dctx::HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 },
        dctx::SelectionPolicy::from_texts(&dctx::RuleTexts::default()),
    );
    let error = session.snapshot(&SnapshotRequest::immutable()).expect_err("mid");
    assert_eq!(error, DctxError::Cancelled);
    assert_eq!(session.generation_count(), 0);
    assert_ne!(log.lock().expect("log").listed(), [] as [std::string::String; 0]);

    let mut session = open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("later");
    assert_eq!(handle.generation(), 1);
    assert!(session.content_reads() > 0);
}

#[test]
fn pack_publishes_once_and_refuses_git_before_a_read() {
    let shared = common::SharedSource::new(common::repository());
    let log = shared.handle();
    let mut session = open_session(shared);
    let mut request = PackRequest::new();
    request.git_revision = Some("HEAD".to_string());
    let mut sink = VecSink::new();
    let error = session.pack(&request, &mut sink).expect_err("git");
    assert!(matches!(error, DctxError::UnsupportedCapability { capability: "git", .. }));
    assert!(sink.is_empty());
    assert_eq!(log.lock().expect("log").reads(), [] as [std::string::String; 0]);

    request.git_revision = None;
    request.output_format = 7;
    let mut sink = VecSink::new();
    let error = session.pack(&request, &mut sink).expect_err("format");
    assert_eq!(error, DctxError::UnsupportedVersion { found: 7 });
    assert!(sink.is_empty());
    assert_eq!(log.lock().expect("log").reads(), [] as [std::string::String; 0]);

    request.output_format = 2;
    let mut sink = VecSink::new();
    let first = session.pack(&request, &mut sink).expect("first");
    assert_eq!(first.version, "V1");
    assert!(first.published);
    assert!(first.major_reset);
    assert!(first.previous.is_none());
    assert!(first.files >= 1);
    assert!(!sink.is_empty());
    let digest = first.digest.clone();

    let mut again = VecSink::new();
    let second = session.pack(&request, &mut again).expect("second");
    assert!(!second.published);
    assert_eq!(second.version, "V1");
    assert_eq!(second.digest, digest);
    assert_eq!(second.previous.as_deref(), Some("V1"));
}

#[test]
fn continuation_pages_and_a_foreign_cursor_is_stale() {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", b"fn alpha() {}\nfn beta() {}\n", 0).expect("lib");
    let mut session = open_session(source);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let mut request = MapRequest::new(Verb::Map);
    request.target = "m:src/lib.rs".to_string();
    request.limits.records = Some(1);
    let mut sink = VecSink::new();
    let first = session.query(&handle, &request, &mut sink).expect("first");
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].id, "s:src/lib.rs#alpha");
    assert_eq!(first.more, 1);
    let cursor = first.next.expect("cursor");

    request.continuation = Some(cursor);
    let mut sink = VecSink::new();
    let second = session.query(&handle, &request, &mut sink).expect("second");
    assert_eq!(second.records.len(), 1);
    assert_eq!(second.records[0].id, "s:src/lib.rs#beta");
    assert_eq!(second.more, 0);
    assert!(second.next.is_none());

    request.continuation = Some("v1|9|map|s:src/lib.rs#alpha".to_string());
    let mut sink = VecSink::new();
    let error = session.query(&handle, &request, &mut sink).expect_err("generation");
    assert_eq!(error, DctxError::StaleContinuation);
    assert!(sink.is_empty());

    request.continuation = Some(format!("v1|{}|find|s:src/lib.rs#alpha", handle.generation()));
    let mut sink = VecSink::new();
    let error = session.query(&handle, &request, &mut sink).expect_err("verb");
    assert_eq!(error.code(), ErrorCode::InvalidRequest);
    assert!(sink.is_empty());
}

#[test]
fn cache_gc_respects_pins_and_purge_requires_confirm() {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", PING, 0).expect("lib");
    let mut session = open_session(source);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let cursor = format!("v1|{}|map|s:src/lib.rs#ping", handle.generation());
    let status = session.cache_status();
    assert_eq!(status.reclaimed_bytes, 0);
    assert!(status.pinned_generations >= 1);
    assert!(status.committed_bytes > 0);

    let blocked = session.cache_gc(GcRequest { apply: true }).expect("blocked");
    assert!(blocked.blocked_generations >= 1);
    assert_eq!(blocked.reclaimed_bytes, 0);
    assert_eq!(session.generation_count(), 1);

    let error = session
        .cache_purge(PurgeRequest { confirm: false, apply: true, scope: PurgeScope::All })
        .expect_err("confirm");
    assert_eq!(error.code(), ErrorCode::InvalidRequest);
    assert_eq!(session.generation_count(), 1);

    let dry = session.cache_gc(GcRequest { apply: false }).expect("dry");
    assert_eq!(dry.reclaimed_bytes, 0);
    assert_eq!(session.generation_count(), 1);

    drop(handle);
    let dry = session
        .cache_purge(PurgeRequest { confirm: true, apply: false, scope: PurgeScope::All })
        .expect("purge dry");
    assert_eq!(dry.reclaimed_bytes, 0);
    assert_eq!(session.generation_count(), 1);

    let collected = session.cache_gc(GcRequest { apply: true }).expect("gc");
    assert!(collected.reclaimed_bytes > 0);
    assert_eq!(session.generation_count(), 0);

    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("second");
    assert_eq!(handle.generation(), 2);
    let mut request = MapRequest::new(Verb::Map);
    request.target = "m:src/lib.rs".to_string();
    request.continuation = Some(cursor);
    let mut sink = VecSink::new();
    let error = session.query(&handle, &request, &mut sink).expect_err("stale");
    assert_eq!(error, DctxError::StaleContinuation);
    assert!(sink.is_empty());
}

#[test]
fn limits_deadline_and_cache_quota_refuse_without_a_generation() {
    let tight = OperationLimits { work_units: Some(1), ..OperationLimits::default() };
    let mut session = open_session(common::repository());
    let error = session
        .snapshot(&SnapshotRequest { limits: tight, ..SnapshotRequest::immutable() })
        .expect_err("work");
    assert!(matches!(error, DctxError::ResourceLimit { resource: Resource::Entries, .. }));
    assert_eq!(session.content_reads(), 0);
    assert_eq!(session.generation_count(), 0);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("room");
    assert_eq!(handle.generation(), 1);

    let frozen = OperationLimits { deadline_ms: Some(0), ..OperationLimits::default() };
    let mut session = open_session(ping_only());
    let handle = session
        .snapshot(&SnapshotRequest { limits: frozen, ..SnapshotRequest::immutable() })
        .expect("deadline");
    assert_eq!(handle.generation(), 1);

    let mut session = session_parts(
        ping_only(),
        Arc::new(CancelFlag::new()),
        dctx::HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 1 },
        dctx::SelectionPolicy::from_texts(&dctx::RuleTexts::default()),
    );
    let error = session.snapshot(&SnapshotRequest::immutable()).expect_err("cache");
    assert!(matches!(error, DctxError::CacheLimit { available: 1, .. }));
    assert_eq!(session.generation_count(), 0);
}

#[test]
fn install_plan_and_revise_selection_bump_the_epoch() {
    let mut session = open_session(ping_only());
    assert_eq!(session.epoch(), 1);
    let plan = session.compile_template(STRUCTURAL).expect("plan");
    session.install_plan(plan);
    assert_eq!(session.epoch(), 2);
    session.revise_selection(dctx::SelectionPolicy::from_texts(&dctx::RuleTexts::default()));
    assert_eq!(session.epoch(), 3);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    assert_eq!(handle.epoch(), 3);
}

const STRUCTURAL: &str = "\
[DTS]
version = 2
[SELECT:s]
roots = [\"src\"]
[INDEX:s]
parser = \"structural\"
[BUDGET:b]
records = 4
[VIEW:map]
selection = \"s\"
index = \"s\"
budget = \"b\"
";

fn ping_only() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("src/lib.rs", PING, 0).expect("lib");
    source
}

#[test]
fn a_segment_written_before_the_v2_parser_id_is_rebuilt_not_reused() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let mut session = open_with_cache(markdown_source(b"# Title\n\nSome text.\n"), &cache);
    let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
    assert!(!handle.reused());
    assert_eq!(handle.generation(), 1);
    let cold_reads = session.content_reads();
    assert!(cold_reads >= 1);
    drop(handle);
    drop(session);

    // An upgraded binary must not reuse a segment an older binary wrote with the structural v1 parser id.
    let patched = patch_persisted_parser(&cache, dctx::STRUCTURAL_PARSER.as_bytes(), b"dctx-structural/1");
    assert!(patched, "the persisted segment carries no structural parser id");

    let mut warm = open_with_cache(markdown_source(b"# Title\n\nSome text.\n"), &cache);
    let handle = warm.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("warm");
    assert!(!handle.reused());
    assert_eq!(warm.cached_content_reads(), 0);
    assert!(warm.content_reads() >= cold_reads);
    drop(handle);
    drop(warm);

    // The same segment, left unpatched, must be reused on a warm start so the parser id is the gate.
    let mut warm2 = open_with_cache(markdown_source(b"# Title\n\nSome text.\n"), &cache);
    let handle = warm2.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("warm2");
    assert!(handle.reused());
    assert!(warm2.cached_content_reads() >= 1);
    assert_eq!(warm2.content_reads(), 0);
}

/// One Markdown file, whose generation keeps the structural parser id on every feature set.
fn markdown_source(body: &[u8]) -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("doc.md", body, 9).expect("file");
    source
}

/// A session with a cache root and auto-clean disabled, so a warm start can load the retained segment.
#[allow(clippy::field_reassign_with_default, reason = "Start from the public limit defaults and disable only auto-clean for the regression.")]
fn open_with_cache(source: MemorySource, cache: &std::path::Path) -> dctx::Session {
    let mut auto = dctx::AutoCleanConfig::default();
    auto.enabled = false;
    dctx::Session::builder(source)
        .clock(dctx::memory::FixedClock(dctx::Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(dctx::SelectionPolicy::from_texts(&dctx::RuleTexts::default()))
        .ceiling(dctx::HostCeiling::base())
        .grants(dctx::HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .place("fixture")
        .view("default")
        .cache_root(cache, 1)
        .auto_clean(auto)
        .build()
        .expect("session")
}

/// Rewrites the parser id inside the persisted segment, simulating the bytes an older binary wrote.
fn patch_persisted_parser(cache: &std::path::Path, from: &[u8], to: &[u8]) -> bool {
    let entries = std::fs::read_dir(cache.join("objects")).expect("objects");
    let mut patched = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(mut bytes) = std::fs::read(&path) else {
            continue;
        };
        if !bytes.starts_with(b"dctxgen1") {
            continue;
        }
        let Some(position) = bytes.windows(from.len()).position(|window| window == from) else {
            continue;
        };
        bytes[position..position + from.len()].copy_from_slice(to);
        std::fs::write(&path, &bytes).expect("patch");
        patched = true;
    }
    patched
}
