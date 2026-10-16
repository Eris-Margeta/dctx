//! Reopened production-path freshness, privacy and warm-query evidence.
//! Implements CM-6c and SPEC-MAP-MODE section 7 with actual filesystem reads counted.
//! Tests use the same Index::open/query/render path as the CLI, never a prepared answer.
use dctx_map_query::ports::Source;
use dctx_map_query::{FileSource, Format, Index, Request, Verb, render_answer};
use std::{
    fs,
    io::Write,
    time::{Duration, Instant},
};

fn fixture(count: usize) -> (tempfile::TempDir, tempfile::TempDir) {
    let source = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    for i in 0..count {
        fs::write(
            source.path().join(format!("file_{i:04}.rs")),
            format!("/// Performs operation {i}.\npub fn operation_{i:04}() {{}}\n"),
        )
        .unwrap();
    }
    (source, cache)
}

fn query(root: &std::path::Path, db: &std::path::Path) -> (String, usize, usize, usize, Duration) {
    let source = FileSource::new(root).unwrap();
    let start = Instant::now();
    let mut index = Index::open("fixture", db).unwrap();
    let request = Request::new(Verb::Find, "operation");
    let answer = index.query(&source, &request).unwrap();
    let output = render_answer(&answer, &request, Format::Text).unwrap();
    (
        output,
        source.content_reads(),
        index.report().parsed,
        index.report().replaced,
        start.elapsed(),
    )
}

#[test]
fn reopened_500_file_query_reads_no_content_and_does_no_reindex() {
    let (root, cache) = fixture(500);
    let db = cache.path().join("index.sqlite");
    let first = query(root.path(), &db);
    assert_eq!(first.1, 500);
    assert_eq!(first.2, 500);
    let second = query(root.path(), &db);
    assert_eq!(second.0, first.0, "CLI output must remain byte-identical");
    assert_eq!((second.1, second.2, second.3), (0, 0, 0));
    let mut samples = vec![second.4];
    for _ in 0..19 {
        let result = query(root.path(), &db);
        assert_eq!((result.1, result.2, result.3), (0, 0, 0));
        assert_eq!(result.0, first.0);
        samples.push(result.4);
    }
    samples.sort();
    let p95 = samples[18];
    fs::write(
        root.path().join("file_0000.rs"),
        "/// Performs changed operation.\npub fn operation_0000() { let changed = 1; }\n",
    )
    .unwrap();
    let changed = query(root.path(), &db);
    assert_eq!((changed.1, changed.2, changed.3), (1, 1, 1));
    writeln!(std::io::stdout().lock(),
        "CM-6c 500 files: cold={:?}, reopened_query_p95={p95:?}, one_file={:?}; warm_reads=0, warm_reindexes=0",
        first.4, changed.4).unwrap();
    if std::env::var_os("DCTX_MAP_PERF").is_some() {
        assert!(p95 < Duration::from_millis(30));
        assert!(changed.4 < Duration::from_millis(50));
    }
}

#[test]
fn five_thousand_file_prefilter_reads_no_content() {
    let (root, _cache) = fixture(5000);
    let source = FileSource::new(root.path()).unwrap();
    let cold = source.snapshot().unwrap();
    let reads = source.content_reads();
    let start = Instant::now();
    let warm = source.snapshot_since(Some(&cold)).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(warm.files.len(), 5000);
    assert_eq!(source.content_reads(), reads);
    writeln!(
        std::io::stdout().lock(),
        "CM-6c 5k pre-filter={elapsed:?}; content_reads=0"
    )
    .unwrap();
    if std::env::var_os("DCTX_MAP_PERF").is_some() {
        assert!(elapsed < Duration::from_millis(20));
    }
}

#[test]
fn touches_update_metadata_without_reindex_and_policy_changes_revoke_content() {
    let (root, cache) = fixture(2);
    let db = cache.path().join("index.sqlite");
    query(root.path(), &db);
    let path = root.path().join("file_0000.rs");
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime + Duration::from_secs(1))
        .unwrap();
    let touched = query(root.path(), &db);
    assert_eq!((touched.1, touched.2, touched.3), (1, 0, 0));
    assert_eq!(query(root.path(), &db).1, 0);
    fs::write(root.path().join(".gitignore"), "file_0000.rs\n").unwrap();
    let result = query(root.path(), &db);
    assert!(!result.0.contains("s:file_0000.rs"));
    assert!(result.0.contains("s:file_0001.rs"));
    assert_eq!(query(root.path(), &db).1, 0);
    fs::write(root.path().join(".gitignore"), "").unwrap();
    assert!(query(root.path(), &db).0.contains("s:file_0000.rs"));
}

#[test]
fn additions_deletions_and_preserved_mtime_edits_survive_reopening() {
    let (root, cache) = fixture(2);
    let db = cache.path().join("index.sqlite");
    query(root.path(), &db);
    let path = root.path().join("file_0000.rs");
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("operation_0000", "operation_9999");
    fs::write(&path, text).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    let result = query(root.path(), &db);
    assert_eq!(result.2, 1);
    assert!(result.0.contains("operation_9999"));
    fs::remove_file(root.path().join("file_0001.rs")).unwrap();
    fs::create_dir(root.path().join("new")).unwrap();
    fs::write(
        root.path().join("new/added.rs"),
        "pub fn operation_added() {}",
    )
    .unwrap();
    let result = query(root.path(), &db);
    assert!(!result.0.contains("s:file_0001.rs"));
    assert!(result.0.contains("operation_added"));
}

#[test]
fn failed_parse_is_retried_after_reopening_without_serving_stale_source() {
    let (root, cache) = fixture(1);
    let db = cache.path().join("index.sqlite");
    query(root.path(), &db);
    fs::write(root.path().join("file_0000.rs"), "pub fn broken(").unwrap();
    let first = query(root.path(), &db);
    let second = query(root.path(), &db);
    assert_eq!(first.1, 1);
    assert_eq!(second.1, 1);
    assert!(second.0.contains("stale"));
    fs::write(
        root.path().join("file_0000.rs"),
        "pub fn operation_fixed() {}",
    )
    .unwrap();
    assert!(query(root.path(), &db).0.contains("operation_fixed"));
}
