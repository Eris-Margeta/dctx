// SPDX-License-Identifier: Apache-2.0
//! Same-stamp edits are invisible to the metadata pre-filter and visible in strict mode.
//! Implements DX-C5 and the DX-B3 strict freshness check.
//! Metadata mode is the positive control: a preserved stamp is not reread.
use dctx_map_query::ports::Source;
use dctx_map_query::{FileSource, Index, Request, Verb};
use std::fs;

#[test]
fn metadata_trusts_a_preserved_stamp_and_strict_mode_sees_the_new_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib.rs");
    fs::write(&path, "pub fn work() { let answer = 42; }\n").unwrap();
    let metadata = FileSource::new(dir.path()).unwrap();
    let mut index = Index::in_memory("strict").unwrap();
    index.refresh(&metadata, None).unwrap();
    let stamp = metadata.snapshot().unwrap().files["lib.rs"].stamp.clone();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, "pub fn work() { let answer = 43; }\n").unwrap();
    let file = fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(modified).unwrap();
    drop(file);
    let after = metadata.snapshot().unwrap();
    assert_eq!(after.files["lib.rs"].stamp, stamp);
    assert_ne!(after.files["lib.rs"].hash, "");
    assert_eq!(
        after.files["lib.rs"].hash,
        metadata.snapshot().unwrap().files["lib.rs"].hash
    );
    let reads = metadata.content_reads();
    let hidden = index
        .query(&metadata, &Request::new(Verb::Read, "s:lib.rs#work"))
        .unwrap();
    assert!(
        hidden.records.is_empty() || hidden.reason.as_deref() == Some("stale: source changed after indexing"),
        "{hidden:?}"
    );
    assert_eq!(metadata.content_reads(), reads + 1);

    let strict = FileSource::strict(dir.path()).unwrap();
    let seen = index
        .query(&strict, &Request::new(Verb::Read, "s:lib.rs#work"))
        .unwrap();
    assert_eq!(index.report().parsed, 1);
    assert!(seen.records[0].body.as_deref().unwrap().contains("43"));
}
