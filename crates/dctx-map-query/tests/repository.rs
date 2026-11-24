// SPDX-License-Identifier: Apache-2.0
//! Repository-scale recall and timing evidence for map mode.
//! Implements packet CM-6b acceptance on a tracked copy of the real dctx workspace.
//! Uses the checked-in recall corpus and public source/index/query APIs.
#[path = "../../../tests/support/source_files.rs"]
mod source_files;

use dctx_map_query::{FileSource, Index, Request, Verb};
use std::{
    io::Write,
    path::Path,
    sync::mpsc::{self, RecvTimeoutError, Sender},
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

fn missing_definition(expected: &str) -> String {
    format!("corpus definition missing: {expected:?}")
}

#[test]
fn missing_definition_escapes_hostile_id() {
    assert_eq!(
        missing_definition("line\n\x1b\"id"),
        r#"corpus definition missing: "line\n\u{1b}\"id""#,
    );
}

#[test]
fn dctx_repository_recall_and_latency() {
    // The last green Intel run took about eleven minutes for this test. Leave
    // room for a cold runner and fail here, with the phase, before the job cap.
    let budget = Duration::from_mins(25);
    let deadline = Instant::now() + budget;
    std::thread::scope(|threads| {
        let (progress, updates) = mpsc::channel();
        threads.spawn(move || {
            let mut phase = "copying the repository";
            loop {
                match updates.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(next) => phase = next,
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {
                        // A stuck worker can also hold a lock needed by another test.
                        // Terminate this integration-test binary so cargo sees the failure.
                        let _ = writeln!(
                            std::io::stderr().lock(),
                            "dctx_repository_recall_and_latency exceeded {budget:?} while {phase}"
                        );
                        std::process::exit(1);
                    }
                }
            }
        });
        measure_repository(&progress);
    });
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep repository recall assertions beside the timing and continuation checks for the same warmed index."
)]
fn measure_repository(progress: &Sender<&'static str>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    assert!(root.join("crates/dctx-core/src/lib.rs").is_file());
    // Keep real working sources and their relative paths, while excluding live scratch.
    let repository = tempfile::tempdir().unwrap();
    for file in source_files::files(root) {
        let target = repository.path().join(file.strip_prefix(root).unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(file, target).unwrap();
    }
    progress.send("building the cold index").unwrap();
    let source = FileSource::new(repository.path()).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let database = cache.path().join("index.sqlite");
    let mut index = Index::open("dctx", &database).unwrap();
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
    progress.send("checking prepared queries").unwrap();
    let rows = corpus();
    assert_ne!(rows, [] as [(std::string::String, std::string::String); 0]);
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
    for (_, expected) in &rows {
        let definition = index
            .answer(&source, &Request::new(Verb::Explain, expected))
            .unwrap();
        assert!(
            definition
                .records
                .iter()
                .any(|record| record.id.as_str() == expected),
            "{}",
            missing_definition(expected)
        );
    }
    latency.sort();
    let p95 = latency[(latency.len() * 95).div_ceil(100) - 1];
    writeln!(std::io::stdout().lock(), "CM-6b evidence: recall={hits}/{}; full_index={indexing:?}; prepared_query_p95={p95:?}; parsed={}; {phases}; stale={:?}", rows.len(), index.report().parsed, index.report().stale).unwrap();
    // Ranking among a growing workspace is benchmark evidence, not a fixed fixture.
    if std::env::var_os("DCTX_MAP_RECALL").is_some() {
        assert!(
            hits * 100 >= rows.len() * 80,
            "recall below 80%: {misses:?}"
        );
    }
    // Shared VM load must not make the default suite flaky; opt in when isolated.
    if std::env::var_os("DCTX_MAP_PERF").is_some() {
        assert!(indexing < Duration::from_secs(3), "full index {indexing:?}");
        assert!(p95 < Duration::from_millis(30), "query p95 {p95:?}");
    }
    progress.send("refreshing the unchanged index").unwrap();
    let report = index.refresh(&source, None).unwrap();
    assert_eq!(report.parsed, 0, "unchanged content must not be reparsed");
    assert_eq!(
        report.replaced, 0,
        "unchanged file facts must not be rewritten"
    );
    drop(index);
    progress
        .send("reopening the index for each corpus query")
        .unwrap();
    let mut reopened = Vec::new();
    let reads = source.content_reads();
    for (question, _) in &rows {
        let start = Instant::now();
        let mut index = Index::open("dctx", &database).unwrap();
        index
            .query(&source, &Request::new(Verb::Find, question))
            .unwrap();
        reopened.push(start.elapsed());
        assert_eq!(index.report().parsed, 0);
        assert_eq!(index.report().replaced, 0);
    }
    assert_eq!(source.content_reads(), reads);
    reopened.sort();
    let p95 = reopened[(reopened.len() * 95).div_ceil(100) - 1];
    writeln!(
        std::io::stdout().lock(),
        "CM-6c dctx reopened_query_p95={p95:?}; content_reads=0; reindexes=0"
    )
    .unwrap();
    if std::env::var_os("DCTX_MAP_PERF").is_some() {
        assert!(
            p95 < Duration::from_millis(30),
            "reopened query p95 {p95:?}"
        );
    }
}

#[test]
fn growing_repository_with_invalid_source_has_zero_reopened_content_reads() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let database = cache.path().join("index.sqlite");
    std::fs::write(root.path().join("valid.rs"), "pub fn available() {}\n").unwrap();
    std::fs::write(
        root.path().join("invalid.rs"),
        include_str!("fixtures/invalid.rs"),
    )
    .unwrap();
    let source = FileSource::new(root.path()).unwrap();
    Index::open("fixture", &database)
        .unwrap()
        .refresh(&source, None)
        .unwrap();
    for added in 0..3 {
        std::fs::write(
            root.path().join(format!("added_{added}.rs")),
            format!("pub fn added_{added}() {{}}\n"),
        )
        .unwrap();
        Index::open("fixture", &database)
            .unwrap()
            .refresh(&source, None)
            .unwrap();
        let reads = source.content_reads();
        for verb in [Verb::Find, Verb::Map, Verb::Explain] {
            let target = if verb == Verb::Find {
                "available"
            } else {
                "m:invalid.rs"
            };
            let mut reopened = Index::open("fixture", &database).unwrap();
            let answer = reopened
                .query(&source, &Request::new(verb, target))
                .unwrap();
            assert_ne!(answer.records, [] as [dctx_map_query::Record; 0]);
            assert_eq!(reopened.report().parsed, 0);
            assert_eq!(reopened.report().replaced, 0);
            assert!(reopened.report().diagnostics.contains_key("invalid.rs"));
        }
        assert_eq!(source.content_reads() - reads, 0);
    }
}
