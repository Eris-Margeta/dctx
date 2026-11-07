// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::domain::{
    DctxError, FreshnessMode, HostGrants, OperationLimits, ParserChoice, PolicyView,
};
use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_ports::memory::{CancelFlag, MemorySource};
use dctx_template::HostCeiling;
use std::fmt::Write as _;

#[test]
fn excluded_git_is_not_read() {
    let mut source = MemorySource::new();
    source.insert_dir(".git", 0).expect("dir");
    source
        .insert_file(".git/config", b"secret", 0)
        .expect("git");
    source
        .insert_file("src/lib.rs", b"fn ping() {}\n", 0)
        .expect("src");
    let selection = SelectionPolicy::from_texts(&RuleTexts::default());
    let ceiling = HostCeiling::base();
    let limits = crate::domain::resolve_scan(&OperationLimits::default()).expect("limits");
    let walked = walk_source(&WalkArgs {
        source: &source,
        cancel: &CancelFlag::new(),
        policy: PolicyView {
            selection: &selection,
            ceiling: &ceiling,
            grants: HostGrants::PermitAll,
            plan: None,
            view: "map",
        },
        limits: &limits,
        freshness: FreshnessMode::ImmutableSnapshot,
        parser: ParserChoice::Structural,
        deadline_ms: None,
        started_ms: 0,
        now_ms: 0,
    })
    .expect("walk");
    assert!(
        walked
            .files
            .iter()
            .all(|file| !file.path.starts_with(".git"))
    );
    assert_eq!(source.reads(), vec!["src/lib.rs".to_string()]);
}

#[test]
fn profiled_files_report_bytes_symbols_and_leave_finalization_out() {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            profile::discard();
        }
    }
    let _reset = Reset;
    profile::enable();
    let mut source = MemorySource::new();
    let mut expected = Vec::new();
    for count in 1..=5 {
        let mut body = String::new();
        for index in 0..count {
            let _ = writeln!(body, "fn f{index}() {{}}");
        }
        let path = format!("f{count}.rs");
        source.insert_file(&path, body.as_bytes(), 0).expect("file");
        expected.push((
            path,
            body.len() as u64,
            u64::try_from(count).expect("nonnegative fixture count"),
        ));
    }
    let selection = SelectionPolicy::from_texts(&RuleTexts::default());
    let ceiling = HostCeiling::base();
    let limits = crate::domain::resolve_scan(&OperationLimits::default()).expect("limits");
    let walked = walk_source(&WalkArgs {
        source: &source,
        cancel: &CancelFlag::new(),
        policy: PolicyView {
            selection: &selection,
            ceiling: &ceiling,
            grants: HostGrants::PermitAll,
            plan: None,
            view: "map",
        },
        limits: &limits,
        freshness: FreshnessMode::ImmutableSnapshot,
        parser: ParserChoice::Structural,
        deadline_ms: None,
        started_ms: 0,
        now_ms: 0,
    })
    .expect("walk");
    assert_eq!(walked.files.len(), 5);
    profile::add_ns(Stage::CanonicalizeSeal, 1_000);
    profile::add_ns(Stage::PersistReuse, 2_000);
    profile::add_ns(Stage::Query, 4_000);
    let report = profile::report().expect("report");
    let mut files = Vec::new();
    let mut stages = Vec::new();
    for line in report.lines() {
        if let Some(rest) = line.strip_prefix("file ") {
            let mut parts = rest.splitn(4, ' ');
            let nanos: u128 = parts
                .next()
                .and_then(|value| value.parse().ok())
                .expect("file ns");
            let bytes: u64 = parts
                .next()
                .and_then(|value| value.parse().ok())
                .expect("bytes");
            let symbols: u64 = parts
                .next()
                .and_then(|value| value.parse().ok())
                .expect("symbols");
            let path = parts.next().expect("path").to_string();
            files.push((nanos, bytes, symbols, path));
        } else if let Some(rest) = line.strip_prefix("stage ") {
            let (name, value) = rest.rsplit_once(' ').expect("stage");
            stages.push((name.to_string(), value.parse::<u128>().expect("stage ns")));
        }
    }
    let names: Vec<_> = stages.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "walk",
            "read",
            "hash",
            "parse/lex",
            "span conversion",
            "graph assembly",
            "canonicalize/seal",
            "persist/reuse",
            "query",
        ]
    );
    assert!(
        files.windows(2).all(|pair| pair[0].0 >= pair[1].0),
        "{report}"
    );
    assert_eq!(files.len(), 5, "{report}");
    let mut seen: Vec<_> = files
        .iter()
        .map(|(_, bytes, symbols, path)| (path.clone(), *bytes, *symbols))
        .collect();
    seen.sort();
    expected.sort();
    assert_eq!(seen, expected, "{report}");
    let file_sum: u128 = files.iter().map(|row| row.0).sum();
    let stage_sum: u128 = stages
        .iter()
        .filter(|(name, _)| {
            matches!(
                name.as_str(),
                "read" | "hash" | "parse/lex" | "span conversion" | "graph assembly"
            )
        })
        .map(|(_, ns)| *ns)
        .sum();
    assert_eq!(file_sum, stage_sum, "{report}");
    assert_eq!(
        stages
            .iter()
            .find(|(name, _)| name == "canonicalize/seal")
            .map(|(_, ns)| *ns),
        Some(1_000)
    );
    assert_eq!(
        stages
            .iter()
            .find(|(name, _)| name == "persist/reuse")
            .map(|(_, ns)| *ns),
        Some(2_000)
    );
    assert_eq!(
        stages
            .iter()
            .find(|(name, _)| name == "query")
            .map(|(_, ns)| *ns),
        Some(4_000)
    );
}

fn fifty_files() -> MemorySource {
    let mut source = MemorySource::new();
    for index in 0..50 {
        let body = format!("fn first{index}() {{}}\nfn second{index}() {{}}\n");
        source
            .insert_file(&format!("f{index:02}.rs"), body.as_bytes(), 0)
            .expect("file");
    }
    source
}

fn walk_capped(
    source: &MemorySource,
    limits: &crate::domain::ScanLimits,
) -> Result<Walked, DctxError> {
    let selection = SelectionPolicy::from_texts(&RuleTexts::default());
    let ceiling = HostCeiling::base();
    walk_source(&WalkArgs {
        source,
        cancel: &CancelFlag::new(),
        policy: PolicyView {
            selection: &selection,
            ceiling: &ceiling,
            grants: HostGrants::PermitAll,
            plan: None,
            view: "map",
        },
        limits,
        freshness: FreshnessMode::ImmutableSnapshot,
        parser: ParserChoice::Structural,
        deadline_ms: None,
        started_ms: 0,
        now_ms: 0,
    })
}

#[test]
fn caps_early_record_cap_refuses_before_the_walk_ends_and_a_raised_cap_passes() {
    let source = fifty_files();
    let mut limits = crate::domain::resolve_scan(&OperationLimits::default()).expect("limits");
    limits.records = 20;
    let error = walk_capped(&source, &limits).expect_err("over the record cap");
    assert_eq!(
        error,
        DctxError::ResourceLimit {
            resource: crate::domain::Resource::Records,
            limit: 20
        }
    );
    assert!(error.to_string().contains("records > 20"), "{error}");
    let reads = source.reads().len();
    assert!(
        reads < 50 && reads > 0,
        "refused after {reads} of 50 reads, not after the whole tree"
    );

    limits.records = 1_000;
    let walked = walk_capped(&fifty_files(), &limits).expect("raised cap");
    assert_eq!(walked.files.len(), 50);
}

#[test]
fn caps_early_entry_cap_refuses_before_the_walk_ends_and_a_raised_cap_passes() {
    let source = fifty_files();
    let mut limits = crate::domain::resolve_scan(&OperationLimits::default()).expect("limits");
    limits.entries = 10;
    let error = walk_capped(&source, &limits).expect_err("over the entry cap");
    assert_eq!(
        error,
        DctxError::ResourceLimit {
            resource: crate::domain::Resource::Entries,
            limit: 10
        }
    );
    let reads = source.reads().len();
    assert!(reads < 50, "refused after {reads} of 50 reads");

    limits.entries = 1_000;
    assert_eq!(
        walk_capped(&fifty_files(), &limits)
            .expect("raised cap")
            .files
            .len(),
        50
    );
}
