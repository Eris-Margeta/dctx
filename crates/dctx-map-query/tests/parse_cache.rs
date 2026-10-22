//! Persistent parse outcomes, recovery labels and invalidation regressions.
//! Implements DX-FIX1 and SPEC-MAP-MODE section 7 through the public query API.
//! SQLite assertions verify the digest, revision and diagnostic survive reopening.
use dctx_map_query::{FileSource, Index, ParseStatus, Request, Verb};
use dctx_map_store::{MapStore, SqliteStore};
use std::{fs, path::Path, time::Duration};

fn payload(database: &Path, path: &str) -> serde_json::Value {
    let index = Index::open("fixture", database).unwrap();
    serde_json::from_str(&index.file_payload(path).unwrap()).unwrap()
}

#[test]
fn failure_has_digest_revision_span_and_only_current_file_node() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let database = cache.path().join("index.sqlite");
    let path = root.path().join("lib.rs");
    fs::write(&path, "pub fn previous() {}\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut index = Index::open("fixture", &database).unwrap();
    index.refresh(&source, None).unwrap();
    fs::write(&path, include_str!("fixtures/invalid.rs")).unwrap();
    index.refresh(&source, None).unwrap();
    let diagnostic = index.report().diagnostics["lib.rs"].clone();
    let ParseStatus::Unparseable {
        diagnostic: ref error,
    } = diagnostic
    else {
        panic!("expected a cached failure: {diagnostic:?}");
    };
    assert_eq!(error.lines, [4, 4]);
    assert!(error.error.contains("invalid syntax"));
    assert!(error.syntax.contains("broken"));
    assert!(index.report().stale.is_empty());
    drop(index);
    let saved = payload(&database, "lib.rs");
    assert_eq!(saved["hash"].as_str().unwrap().len(), 64);
    assert!(saved["parser_revision"].as_u64().unwrap() > 0);
    assert_eq!(saved["status"]["status"], "unparseable");
    let file = SqliteStore::open(&database)
        .unwrap()
        .file("lib.rs")
        .unwrap()
        .unwrap();
    assert_eq!(file.file.sha256, saved["hash"].as_str().unwrap());
    assert_eq!(file.nodes.len(), 1);
    assert_eq!(file.nodes[0].id.as_str(), "m:lib.rs");
    let reads = source.content_reads();
    for _ in 0..2 {
        let mut reopened = Index::open("fixture", &database).unwrap();
        let answer = reopened
            .query(&source, &Request::new(Verb::Map, "lib.rs"))
            .unwrap();
        assert_eq!(answer.records.len(), 1);
        assert!(answer.records[0].sum.starts_with("unparseable:"));
        assert_eq!(reopened.report().diagnostics["lib.rs"], diagnostic);
        assert_eq!(reopened.report().parsed, 0);
        assert_eq!(reopened.report().replaced, 0);
    }
    assert_eq!(source.content_reads(), reads);
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime + Duration::from_secs(2))
        .unwrap();
    let mut index = Index::open("fixture", &database).unwrap();
    index.refresh(&source, None).unwrap();
    assert_eq!(source.content_reads(), reads + 1);
    assert_eq!(index.report().parsed, 0, "same digest reuses the failure");
    drop(index);
    fs::write(&path, "pub fn repaired() {}\n").unwrap();
    let mut index = Index::open("fixture", &database).unwrap();
    index.refresh(&source, None).unwrap();
    assert_eq!(index.report().parsed, 1);
    assert!(index.report().diagnostics.is_empty());
    assert_ne!(
        index
            .answer(&source, &Request::new(Verb::Explain, "s:lib.rs#repaired"))
            .unwrap()
            .records, [] as [dctx_map_query::Record; 0]
    );
}

#[test]
fn revision_change_reparses_once_even_with_unchanged_source_metadata() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let database = cache.path().join("index.sqlite");
    fs::write(
        root.path().join("lib.rs"),
        include_str!("fixtures/invalid.rs"),
    )
    .unwrap();
    let source = FileSource::new(root.path()).unwrap();
    Index::open("fixture", &database)
        .unwrap()
        .refresh(&source, None)
        .unwrap();
    let mut index = Index::open("fixture", &database).unwrap();
    let current: serde_json::Value =
        serde_json::from_str(&index.file_payload("lib.rs").unwrap()).unwrap();
    // Also exercises migration of caches written before parser revisions existed.
    let mut stripped = current.clone();
    let object = stripped.as_object_mut().unwrap();
    object.remove("parser_revision");
    object.remove("status");
    index
        .replace_file_payload("lib.rs", &stripped.to_string())
        .unwrap();
    drop(index);
    let reads = source.content_reads();
    let mut upgraded = Index::open("fixture", &database).unwrap();
    upgraded.refresh(&source, None).unwrap();
    assert_eq!(upgraded.report().parsed, 1);
    assert_eq!(source.content_reads(), reads + 1);
    assert_eq!(payload(&database, "lib.rs"), current);
    drop(upgraded);
    let mut reopened = Index::open("fixture", &database).unwrap();
    reopened.refresh(&source, None).unwrap();
    assert_eq!(reopened.report().parsed, 0);
    assert_eq!(source.content_reads(), reads + 1);
}

#[test]
fn deadline_failure_remains_retryable_after_reopening() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let database = cache.path().join("index.sqlite");
    let path = root.path().join("lib.rs");
    fs::write(&path, "pub fn before() {}\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut index = Index::open("fixture", &database).unwrap();
    index.refresh(&source, None).unwrap();
    fs::write(&path, "pub fn after() {}\n").unwrap();
    index.refresh(&source, Some(Duration::ZERO)).unwrap();
    assert!(index.report().stale.contains_key("lib.rs"));
    assert!(index.report().diagnostics.is_empty());
    let read = index
        .answer(&source, &Request::new(Verb::Read, "s:lib.rs#before"))
        .unwrap();
    assert_eq!(read.records, [] as [dctx_map_query::Record; 0]);
    drop(index);
    let mut reopened = Index::open("fixture", &database).unwrap();
    reopened.refresh(&source, None).unwrap();
    assert_eq!(reopened.report().parsed, 1);
    assert!(reopened.report().stale.is_empty());
    let answer = reopened
        .answer(&source, &Request::new(Verb::Explain, "s:lib.rs#after"))
        .unwrap();
    assert_eq!(answer.records.len(), 1);
}
