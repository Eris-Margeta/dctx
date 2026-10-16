//! Byte parity for lazily loaded persistent graph queries and crash repair.
//! Implements CM-6c, preserving all five verbs while changing cache hydration.
//! Compares public render output against the complete in-memory reference graph.
use dctx_map_query::{
    Direction, EdgeKind, FileSource, Format, Index, Request, Verb, render_answer,
};
use dctx_map_store::{MapStore, SqliteStore};
use std::fs;

#[test]
fn reopened_verbs_are_byte_identical_to_the_complete_graph() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname=\"fixture\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "mod job;\npub fn caller() { job::work(); }\n#[test]\nfn checks_work() { job::work(); }",
    )
    .unwrap();
    fs::write(
        root.path().join("src/job.rs"),
        "/// Performs work.\npub fn work() {}",
    )
    .unwrap();
    fs::write(root.path().join("notes.md"), "# Notes\nSee `src/job.rs`.").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut memory = Index::in_memory("fixture").unwrap();
    memory.refresh(&source, None).unwrap();
    Index::open("fixture", &db)
        .unwrap()
        .refresh(&source, None)
        .unwrap();
    let mut requests = vec![
        Request::new(Verb::Find, "work"),
        Request::new(Verb::Explain, "s:src/job.rs#work"),
        Request::new(Verb::Read, "s:src/job.rs#work"),
        Request::new(Verb::Explain, "s:src/job.rs#gone"),
    ];
    for target in ["", ".", "src", "src/job.rs", "m:src/job.rs", "k:."] {
        for depth in 1..=3 {
            let mut request = Request::new(Verb::Map, target);
            request.depth = depth;
            requests.push(request);
        }
    }
    for direction in [Direction::In, Direction::Out, Direction::Both] {
        for depth in 1..=2 {
            let mut request = Request::new(Verb::Neighbors, "s:src/job.rs#work");
            request.depth = depth;
            request.direction = direction;
            requests.push(request.clone());
            request.edges = vec![EdgeKind::Calls, EdgeKind::Tests];
            requests.push(request);
        }
    }
    for request in requests {
        let mut disk = Index::open("fixture", &db).unwrap();
        let expected = memory.answer(&source, &request).unwrap();
        let actual = disk.query(&source, &request).unwrap();
        for format in [Format::Text, Format::Json] {
            assert_eq!(
                render_answer(&actual, &request, format).unwrap(),
                render_answer(&expected, &request, format).unwrap(),
                "{request:?}"
            );
        }
        assert_eq!(disk.report().replaced, 0);
    }
}

#[test]
fn interrupted_file_transactions_repair_before_a_new_reader_answers() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let db = cache.path().join("index.sqlite");
    fs::write(root.path().join("lib.rs"), "pub fn intact() {}").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let request = Request::new(Verb::Find, "intact");
    Index::open("fixture", &db)
        .unwrap()
        .query(&source, &request)
        .unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute("UPDATE query_cache SET writing=1", [])
        .unwrap();
    let mut store = SqliteStore::open(&db).unwrap();
    let mut file = store.file("lib.rs").unwrap().unwrap();
    file.nodes.clear();
    file.edges.clear();
    store.reindex(&file).unwrap();
    drop(store);
    drop(connection);
    let mut repaired = Index::open("fixture", &db).unwrap();
    let answer = repaired.query(&source, &request).unwrap();
    assert!(
        answer
            .records
            .iter()
            .any(|r| r.id.as_str() == "s:lib.rs#intact")
    );
    assert_eq!(repaired.report().parsed, 0);
    assert_eq!(repaired.report().replaced, 1);
    assert_eq!(
        Index::open("fixture", &db)
            .unwrap()
            .query(&source, &request)
            .unwrap()
            .records,
        answer.records
    );
}
