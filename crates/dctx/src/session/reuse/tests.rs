use std::sync::Arc;

use dctx_core::{SelectionPolicy, Timestamp};
use dctx_ports::memory::{CancelFlag, FixedClock, MemorySource};

use super::super::SnapshotHandle;
use super::*;
use crate::domain::{AutoCleanConfig, HostGrants, MapRequest, VecSink, Verb};
use crate::{HostCeiling, RuleTexts, StoragePolicy};

#[allow(clippy::field_reassign_with_default, reason = "Start from the public limit defaults and change only the limit exercised by this regression.")]
fn open(source: MemorySource, cache: &std::path::Path) -> Session {
    let mut auto = AutoCleanConfig::default();
    auto.enabled = false;
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .place("fixture")
        .view("default")
        .cache_root(cache, 1)
        .auto_clean(auto)
        .build()
        .expect("session")
}

fn source(body: &[u8]) -> MemorySource {
    let mut source = MemorySource::new();
    source.insert_file("item.rs", body, 9).expect("file");
    source
}

fn find_ids(session: &mut Session, handle: &SnapshotHandle, query: &str) -> Vec<String> {
    let mut request = MapRequest::new(Verb::Find);
    request.query = query.to_string();
    let mut sink = VecSink::new();
    let answer = session.query(handle, &request, &mut sink).expect("query");
    answer.records.into_iter().map(|record| record.id).collect()
}

#[test]
fn same_stamp_byte_change_rebuilds_and_a_match_reuses() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let mut session = open(source(b"fn kept() {}\n"), &cache);
    let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
    assert!(!handle.reused());
    assert!(session.content_reads() >= 1);
    assert_eq!(session.cached_content_reads(), 0);
    let cold_reads = session.content_reads();
    let kept = find_ids(&mut session, &handle, "kept");
    assert!(kept.iter().any(|id| id.contains("kept")), "{kept:?}");
    drop(handle);
    drop(session);

    let mut warm = open(source(b"fn kept() {}\n"), &cache);
    let handle = warm.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("warm");
    assert!(handle.reused());
    assert_eq!(handle.generation(), 1);
    assert_eq!(warm.content_reads(), 0);
    assert!(warm.content_reads() < cold_reads);
    assert!(warm.cached_content_reads() >= 1);
    assert_eq!(find_ids(&mut warm, &handle, "kept"), kept);
    drop(handle);
    drop(warm);

    let mut third = open(source(b"fn lost() {}\n"), &cache);
    let handle = third.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("third");
    assert!(!handle.reused());
    assert!(third.content_reads() >= 1);
    let lost = find_ids(&mut third, &handle, "lost");
    assert!(lost.iter().any(|id| id.contains("lost")), "{lost:?}");
    assert_eq!(find_ids(&mut third, &handle, "kept"), [] as [std::string::String; 0]);
}

#[test]
fn a_different_source_key_does_not_reuse() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let mut session = open(source(b"fn kept() {}\n"), &cache);
    let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
    assert!(!handle.reused());
    drop(handle);
    drop(session);

    let mut other = open(source(b"fn kept() {}\n"), &cache);
    let handle = other.snapshot_reusable(&SnapshotRequest::strict(), "/other").expect("other");
    assert!(!handle.reused());
    assert!(other.content_reads() >= 1);
    assert_eq!(other.cached_content_reads(), 0);
}

#[allow(clippy::field_reassign_with_default, reason = "Start from the public limit defaults and change only the limit exercised by this regression.")]
fn open_with_rules(source: MemorySource, cache: &std::path::Path, rules: crate::domain::RuleSources) -> Session {
    let mut auto = AutoCleanConfig::default();
    auto.enabled = false;
    let selection = rules.policy();
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(selection)
        .rule_sources(rules)
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .place("fixture")
        .view("default")
        .cache_root(cache, 1)
        .auto_clean(auto)
        .build()
        .expect("session")
}

#[test]
fn a_comment_only_rule_edit_rebuilds_and_the_same_text_reuses() {
    let dir = tempfile::tempdir().expect("temp");
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).expect("dir");
    let first = crate::domain::RuleSources { local_ignore: Some("# one\n".to_string()), ..crate::domain::RuleSources::default() };
    let mut session = open_with_rules(source(b"fn kept() {}\n"), &cache, first);
    let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
    assert!(!handle.reused());
    drop(handle);
    drop(session);

    let same = crate::domain::RuleSources { local_ignore: Some("# one\n".to_string()), ..crate::domain::RuleSources::default() };
    let mut warm = open_with_rules(source(b"fn kept() {}\n"), &cache, same);
    let handle = warm.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("warm");
    assert!(handle.reused());
    assert_eq!(warm.content_reads(), 0);
    drop(handle);
    drop(warm);

    let edited = crate::domain::RuleSources { local_ignore: Some("# two\n".to_string()), ..crate::domain::RuleSources::default() };
    let mut third = open_with_rules(source(b"fn kept() {}\n"), &cache, edited);
    let handle = third.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("edited");
    assert!(!handle.reused());
    assert!(third.content_reads() >= 1);
}
