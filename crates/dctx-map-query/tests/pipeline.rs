//! Public pipeline tests for incremental indexing and graph traversal.
//! Implements SPEC-MAP-MODE sections 4, 5.2 and 7 using real temporary files.
//! Verifies stale refusals, dependency retries, moves and persisted cache reuse.
use dctx_map_query::{EdgeKind, FileSource, Format, Index, Request, Verb, render_answer};
use std::{fs, time::Duration};

fn fixture() -> (tempfile::TempDir, FileSource, Index) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "mod worker;\n/// Runs the job.\npub fn run() { worker::work(); }\n#[test]\nfn checks_work() { worker::work(); }\n").unwrap();
    fs::write(
        dir.path().join("src/worker.rs"),
        "/// Performs the job.\npub fn work() { let answer = 42; }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("notes.md"),
        "# Jobs\nSee `src/worker.rs`.\n",
    )
    .unwrap();
    let source = FileSource::new(dir.path()).unwrap();
    let mut index = Index::in_memory("sample").unwrap();
    index.refresh(&source, None).unwrap();
    (dir, source, index)
}
#[test]
fn all_five_verbs_share_facts_and_exact_source() {
    let (_dir, source, mut index) = fixture();
    let mut map = Request::new(Verb::Map, "");
    map.depth = 3;
    let outline = index.answer(&source, &map).unwrap();
    assert!(
        outline
            .records
            .iter()
            .any(|r| r.id.as_str() == "s:src/worker.rs#work")
    );
    let find = index
        .query(&source, &Request::new(Verb::Find, "work"))
        .unwrap();
    assert!(
        find.records
            .iter()
            .any(|r| r.id.as_str() == "s:src/worker.rs#work")
    );
    let id = "s:src/worker.rs#work";
    let neighbors = index
        .answer(&source, &Request::new(Verb::Neighbors, id))
        .unwrap();
    assert!(
        neighbors
            .records
            .iter()
            .any(|r| r.id.as_str() == "s:src/lib.rs#run"
                && r.relation.as_ref().unwrap().edge == EdgeKind::Calls)
    );
    let explain = index
        .answer(&source, &Request::new(Verb::Explain, id))
        .unwrap();
    assert!(
        explain.records[0]
            .signature
            .as_ref()
            .unwrap()
            .contains("pub fn work")
    );
    assert!(
        explain.records[0]
            .tests
            .iter()
            .any(|id| id.as_str() == "t:src/lib.rs#checks_work")
    );
    let read = index
        .answer(&source, &Request::new(Verb::Read, id))
        .unwrap();
    assert_eq!(
        read.records[0].body.as_deref(),
        Some("pub fn work() { let answer = 42; }\n")
    );
}
#[test]
fn same_size_edit_is_hashed_and_invalid_source_is_never_served() {
    let (dir, _source, mut index) = fixture();
    let path = dir.path().join("src/worker.rs");
    let old_time = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(
        &path,
        "/// Performs the job.\npub fn work() { let answer = 43; }\n",
    )
    .unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old_time)
        .unwrap();
    let source = FileSource::strict(dir.path()).unwrap();
    let read = index
        .query(&source, &Request::new(Verb::Read, "s:src/worker.rs#work"))
        .unwrap();
    assert!(read.records[0].body.as_ref().unwrap().contains("43"));
    assert_eq!(index.report().parsed, 1);
    fs::write(&path, "pub fn work(").unwrap();
    let invalid = index
        .query(&source, &Request::new(Verb::Find, "work"))
        .unwrap();
    assert!(
        !invalid
            .records
            .iter()
            .any(|r| r.id.as_str() == "s:src/worker.rs#work")
    );
    assert!(index.report().diagnostics.contains_key("src/worker.rs"));
    assert!(index.report().stale.is_empty());
    let read = index
        .answer(&source, &Request::new(Verb::Read, "s:src/worker.rs#work"))
        .unwrap();
    assert!(read.records.is_empty());
    assert!(read.identity.is_some());
    fs::write(&path, "pub fn work() { let recovered = 44; }").unwrap();
    assert!(
        !index
            .query(&source, &Request::new(Verb::Read, "s:src/worker.rs#work"))
            .unwrap()
            .records[0]
            .stale
    );
}
#[test]
fn deadline_marks_changed_files_stale_and_deleted_ids_are_gone() {
    let (dir, source, mut index) = fixture();
    fs::write(
        dir.path().join("src/worker.rs"),
        "pub fn work() { let pending = 45; }",
    )
    .unwrap();
    index.refresh(&source, Some(Duration::ZERO)).unwrap();
    assert!(index.report().stale.contains_key("src/worker.rs"));
    let stale = index
        .answer(&source, &Request::new(Verb::Read, "s:src/worker.rs#work"))
        .unwrap();
    assert!(stale.records.is_empty());
    index.refresh(&source, None).unwrap();
    fs::remove_file(dir.path().join("src/worker.rs")).unwrap();
    let gone = index
        .query(
            &source,
            &Request::new(Verb::Explain, "s:src/worker.rs#work"),
        )
        .unwrap();
    assert!(gone.identity.is_some());
    assert!(gone.records.is_empty());
    assert!(
        render_answer(
            &gone,
            &Request::new(Verb::Explain, "s:src/worker.rs#work"),
            Format::Text
        )
        .unwrap()
        .contains("gone:")
    );
}
#[test]
fn moves_and_retries_survive_reopening() {
    let (dir, source, _) = fixture();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    let mut persistent = Index::open("sample", &db).unwrap();
    persistent.refresh(&source, None).unwrap();
    persistent.save(&db).unwrap();
    drop(persistent);
    fs::rename(
        dir.path().join("src/worker.rs"),
        dir.path().join("src/renamed.rs"),
    )
    .unwrap();
    let mut index = Index::open("sample", &db).unwrap();
    index.refresh(&source, None).unwrap();
    assert_eq!(index.report().parsed, 1);
    let moved = index
        .answer(
            &source,
            &Request::new(Verb::Explain, "s:src/worker.rs#work"),
        )
        .unwrap();
    assert!(moved.identity.is_some());
    assert_eq!(moved.records[0].id.as_str(), "s:src/renamed.rs#work");
    index.save(&db).unwrap();
    drop(index);
    let mut index = Index::open("sample", &db).unwrap();
    index.refresh(&source, None).unwrap();
    assert_eq!(index.report().parsed, 0);
    assert!(
        index
            .answer(
                &source,
                &Request::new(Verb::Explain, "s:src/worker.rs#work")
            )
            .unwrap()
            .identity
            .is_some()
    );
}
#[test]
fn unchanged_callers_are_reresolved_when_missing_definitions_arrive() {
    let (dir, source, mut index) = fixture();
    fs::write(
        dir.path().join("src/lib.rs"),
        "mod future;\npub fn caller() { future::arrive(); }\n",
    )
    .unwrap();
    index.refresh(&source, None).unwrap();
    fs::write(dir.path().join("src/future.rs"), "pub fn arrive() {}\n").unwrap();
    index.refresh(&source, None).unwrap();
    assert_eq!(index.report().parsed, 1);
    let answer = index
        .answer(
            &source,
            &Request::new(Verb::Neighbors, "s:src/lib.rs#caller"),
        )
        .unwrap();
    assert!(
        answer
            .records
            .iter()
            .any(|r| r.id.as_str() == "s:src/future.rs#arrive")
    );
}
