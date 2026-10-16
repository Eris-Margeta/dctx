//! Repository-scale recall and timing evidence for map mode.
//! Implements packet CM-6b acceptance on the real dctx workspace, not synthetic code.
//! Uses the checked-in thirty-question corpus and public source/index/query APIs.
use dctx_map_query::{FileSource, Index, Request, Verb};
use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

fn corpus() -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let mut question = None;
    for line in include_str!("recall.toml").lines() {
        if let Some(text) = line.strip_prefix("text = ") {
            question = Some(serde_json::from_str::<String>(text).unwrap());
        }
        if let Some(id) = line.strip_prefix("expected = ") {
            rows.push((
                question.take().unwrap(),
                serde_json::from_str::<String>(id).unwrap(),
            ));
        }
    }
    rows
}

#[test]
fn dctx_repository_recall_and_latency() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    assert!(root.join("crates/dctx-core/src/lib.rs").is_file());
    let source = FileSource::new(root).unwrap();
    let mut index = Index::in_memory("dctx").unwrap();
    let start = Instant::now();
    index.refresh(&source, None).unwrap();
    let indexing = start.elapsed();
    let phases = format!(
        "scan={:?}, parse={:?}, resolve={:?}, store={:?}",
        index.report().scan_time,
        index.report().parse_time,
        index.report().resolve_time,
        index.report().store_time
    );
    let rows = corpus();
    assert_eq!(rows.len(), 30);
    let mut hits = 0;
    let mut latency = Vec::new();
    let mut misses = Vec::new();
    for (question, expected) in &rows {
        let request = Request::new(Verb::Find, question);
        let start = Instant::now();
        let answer = index.answer(&source, &request).unwrap();
        latency.push(start.elapsed());
        if answer
            .records
            .iter()
            .take(10)
            .any(|r| r.id.as_str() == expected)
        {
            hits += 1;
        } else {
            misses.push((
                question,
                expected,
                answer
                    .records
                    .iter()
                    .map(|r| r.id.to_string())
                    .collect::<Vec<_>>(),
            ));
        }
    }
    latency.sort();
    let p95 = latency[(latency.len() * 95).div_ceil(100) - 1];
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "CM-6b evidence: recall={hits}/30; full_index={indexing:?}; prepared_query_p95={p95:?}; parsed={}; {phases}; stale={:?}", index.report().parsed, index.report().stale).unwrap();
    assert!(hits >= 24, "recall below 80%: {misses:#?}");
    // Shared VM load must not make the default suite flaky; opt in when isolated.
    if std::env::var_os("DCTX_MAP_PERF").is_some() {
        assert!(indexing < Duration::from_secs(3), "full index {indexing:?}");
        assert!(p95 < Duration::from_millis(30), "query p95 {p95:?}");
    }
    let report = index.refresh(&source, None).unwrap();
    assert_eq!(report.parsed, 0, "unchanged content must not be reparsed");
    assert_eq!(
        report.replaced, 0,
        "unchanged file facts must not be rewritten"
    );
}
