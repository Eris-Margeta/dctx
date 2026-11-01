use std::sync::Arc;

use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_core::Timestamp;
use dctx_ports::memory::{CancelFlag, FixedClock, MemorySource};
use dctx_template::HostCeiling;

use super::{Session, StoragePolicy};
use crate::domain::{HostGrants, MapRequest, SnapshotRequest, VecSink, Verb};

#[test]
fn a_query_borrows_the_stored_generation() {
    let mut source = MemorySource::new();
    source.insert_file("src/a.rs", b"fn alpha() {}\n", 0).expect("a");
    source.insert_file("src/b.rs", b"fn beta() {}\n", 0).expect("b");
    let mut session = Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .place("fixture")
        .view("map")
        .build()
        .expect("session");
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let (live, seen) = session.generation_shares(&handle).expect("before");
    assert_eq!((live, seen), (1, 0));
    let mut request = MapRequest::new(Verb::Find);
    request.query = "alpha".to_string();
    let mut sink = VecSink::new();
    let first = session.query(&handle, &request, &mut sink).expect("find");
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].path, "src/a.rs");
    assert_eq!(first.snapshot, handle.digest());
    let (live, seen) = session.generation_shares(&handle).expect("after");
    assert_eq!(live, 1);
    assert_eq!(seen, 2);
    let mut sink = VecSink::new();
    let second = session.query(&handle, &request, &mut sink).expect("again");
    assert_eq!(second.records, first.records);
    assert_eq!(second.snapshot, first.snapshot);
    let (live, seen) = session.generation_shares(&handle).expect("still");
    assert_eq!((live, seen), (1, 2));
}

#[test]
fn a_profiled_snapshot_keeps_finalization_off_the_file_rows() {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            crate::index::profile::discard();
        }
    }
    let _reset = Reset;
    let mut source = MemorySource::new();
    let alpha = b"fn alpha() {}\n";
    let beta = b"fn beta() {}\nfn gamma() {}\n";
    source.insert_file("src/a.rs", alpha, 0).expect("a");
    source.insert_file("src/b.rs", beta, 0).expect("b");
    let mut session = Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .place("fixture")
        .view("map")
        .build()
        .expect("session");
    session.enable_profile();
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let persist = crate::index::profile::stage_ns(crate::index::profile::Stage::PersistReuse);
    session.observe_profile_snapshot(std::time::Duration::from_mins(16650));
    assert_eq!(crate::index::profile::stage_ns(crate::index::profile::Stage::PersistReuse), persist);
    let mut request = MapRequest::new(Verb::Find);
    request.query = "alpha".to_string();
    let mut sink = VecSink::new();
    session.query_for_profile(&handle, &request, &mut sink).expect("find");
    let report = session.profile_report().expect("report");
    for name in [
        "walk",
        "read",
        "hash",
        "parse/lex",
        "span conversion",
        "graph assembly",
        "canonicalize/seal",
        "persist/reuse",
        "query",
    ] {
        assert!(report.contains(&format!("stage {name} ")), "{name}\n{report}");
    }
    let rows: Vec<_> = report
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("file ")?;
            let mut parts = rest.splitn(4, ' ');
            let bytes: u64 = parts.nth(1)?.parse().ok()?;
            let symbols: u64 = parts.next()?.parse().ok()?;
            let path = parts.next()?;
            Some((path.to_string(), bytes, symbols))
        })
        .collect();
    assert!(rows.contains(&("src/a.rs".to_string(), alpha.len() as u64, 1)), "{report}");
    assert!(rows.contains(&("src/b.rs".to_string(), beta.len() as u64, 2)), "{report}");
    assert!(!report.contains("999000000000000"), "{report}");
}
