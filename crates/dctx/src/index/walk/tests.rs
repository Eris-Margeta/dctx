use std::fmt::Write as _;
use super::*;
use crate::domain::{FreshnessMode, HostGrants, OperationLimits, ParserChoice, PolicyView};
use dctx_core::selection::{RuleTexts, SelectionPolicy};
use dctx_ports::memory::{CancelFlag, MemorySource};
use dctx_template::HostCeiling;

#[test]
fn excluded_git_is_not_read() {
    let mut source = MemorySource::new();
    source.insert_dir(".git", 0).expect("dir");
    source.insert_file(".git/config", b"secret", 0).expect("git");
    source.insert_file("src/lib.rs", b"fn ping() {}\n", 0).expect("src");
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
    assert!(walked.files.iter().all(|file| !file.path.starts_with(".git")));
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
        expected.push((path, body.len() as u64, u64::try_from(count).expect("nonnegative fixture count")));
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
            let nanos: u128 = parts.next().and_then(|value| value.parse().ok()).expect("file ns");
            let bytes: u64 = parts.next().and_then(|value| value.parse().ok()).expect("bytes");
            let symbols: u64 = parts.next().and_then(|value| value.parse().ok()).expect("symbols");
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
    assert!(files.windows(2).all(|pair| pair[0].0 >= pair[1].0), "{report}");
    assert_eq!(files.len(), 5, "{report}");
    let mut seen: Vec<_> = files.iter().map(|(_, bytes, symbols, path)| (path.clone(), *bytes, *symbols)).collect();
    seen.sort();
    expected.sort();
    assert_eq!(seen, expected, "{report}");
    let file_sum: u128 = files.iter().map(|row| row.0).sum();
    let stage_sum: u128 = stages
        .iter()
        .filter(|(name, _)| matches!(name.as_str(), "read" | "hash" | "parse/lex" | "span conversion" | "graph assembly"))
        .map(|(_, ns)| *ns)
        .sum();
    assert_eq!(file_sum, stage_sum, "{report}");
    assert_eq!(stages.iter().find(|(name, _)| name == "canonicalize/seal").map(|(_, ns)| *ns), Some(1_000));
    assert_eq!(stages.iter().find(|(name, _)| name == "persist/reuse").map(|(_, ns)| *ns), Some(2_000));
    assert_eq!(stages.iter().find(|(name, _)| name == "query").map(|(_, ns)| *ns), Some(4_000));
}
