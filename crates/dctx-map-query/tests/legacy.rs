// SPDX-License-Identifier: Apache-2.0
//! Version-1 query.json upgrades into a segment generation.
//! Implements the cache migration previously stored beside the sqlite file.
//! Moves survive, and a second open does not reread unchanged source.
use dctx_map_query::{FileSource, Index, Request, Verb};
use std::fs;

#[test]
fn legacy_retry_facts_and_moves_survive_cache_upgrade() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let database = cache.path().join("index.sqlite");
    fs::write(root.path().join("old.rs"), "pub fn work() {}").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut old = Index::in_memory("fixture").unwrap();
    old.refresh(&source, None).unwrap();
    let legacy = serde_json::json!({
        "version": 1,
        "place": "fixture",
        "files": {
            "old.rs": serde_json::from_str::<serde_json::Value>(&old.file_payload("old.rs").unwrap()).unwrap()
        },
        "moves": {"s:earlier.rs#work": "s:old.rs#work"}
    });
    fs::write(
        database.with_extension("query.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    fs::write(cache.path().join("writer.lock"), "abandoned legacy lock").unwrap();
    fs::rename(root.path().join("old.rs"), root.path().join("new.rs")).unwrap();
    let request = Request::new(Verb::Explain, "s:earlier.rs#work");
    let mut upgraded = Index::open("fixture", &database).unwrap();
    let answer = upgraded.query(&source, &request).unwrap();
    assert_eq!(answer.records[0].id.as_str(), "s:new.rs#work");
    drop(upgraded);
    let reads = source.content_reads();
    let mut reopened = Index::open("fixture", &database).unwrap();
    let answer = reopened.query(&source, &request).unwrap();
    assert_eq!(answer.records[0].id.as_str(), "s:new.rs#work");
    assert_eq!(source.content_reads(), reads);
}

#[cfg(unix)]
#[test]
fn linked_cache_ancestors_preserve_save_identity_and_refuse_legacy_file_links() {
    let temp = tempfile::tempdir().unwrap();
    let real = fs::canonicalize(temp.path()).unwrap().join("real");
    fs::create_dir(&real).unwrap();
    let linked = temp.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    let database = linked.join("index.sqlite");
    let legacy = database.with_extension("query.json");
    let bytes = serde_json::to_vec(&serde_json::json!({
        "version": 1, "place": "fixture", "files": {}, "moves": {}
    }))
    .unwrap();
    fs::write(&legacy, &bytes).unwrap();
    let index = Index::open("fixture", &database).unwrap();
    index.save(&database).unwrap();
    assert!(index.save(real.join("index.sqlite")).is_err());
    drop(index);
    let outside = temp.path().join("outside.json");
    fs::write(&outside, &bytes).unwrap();
    fs::remove_file(&legacy).unwrap();
    std::os::unix::fs::symlink(&outside, &legacy).unwrap();
    assert!(Index::open("fixture", &database).is_err());
    assert_eq!(fs::read(outside).unwrap(), bytes);
}
