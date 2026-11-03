// SPDX-License-Identifier: Apache-2.0
//! Rust compatibility recovery and modern-syntax integration regressions.
//! Implements DX-FIX1 pending the syn extractor, preserving source coordinates.
//! Uses temporary repositories and the public query API across persistent reopen.
use dctx_map_query::{FileSource, Index, ParseStatus, Request, Verb};
use std::fs;

#[test]
fn recovery_is_partial_persistent_and_keeps_original_source_spans() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let database = cache.path().join("index.sqlite");
    fs::write(root.path().join("lib.rs"),
        "pub fn before() {}\npub fn unreadable() {\n    let broken = ;\n}\npub fn after() { before(); }\n").unwrap();
    let source = FileSource::new(root.path()).unwrap();
    let mut index = Index::open("fixture", &database).unwrap();
    index.refresh(&source, None).unwrap();
    assert!(matches!(
        index.report().diagnostics["lib.rs"],
        ParseStatus::Partial { .. }
    ));
    assert!(index.report().stale.is_empty());
    drop(index);
    let reads = source.content_reads();
    let mut reopened = Index::open("fixture", &database).unwrap();
    let request = Request::new(Verb::Explain, "s:lib.rs#after");
    let answer = reopened.query(&source, &request).unwrap();
    assert_eq!(answer.records[0].l, [5, 5]);
    assert!(answer.records[0].sum.starts_with("low-precision:"));
    assert!(matches!(
        reopened.report().diagnostics["lib.rs"],
        ParseStatus::Partial { .. }
    ));
    assert_eq!(source.content_reads(), reads);
    let raw = reopened
        .answer(&source, &Request::new(Verb::Read, "s:lib.rs#after"))
        .unwrap();
    assert_eq!(
        raw.records[0].body.as_deref(),
        Some("pub fn after() { before(); }\n")
    );
}

#[test]
fn modern_rust_keeps_available_facts_and_labels_any_recovery() {
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let database = cache.path().join("index.sqlite");
    for (name, modern) in [
        (
            "chains",
            "pub fn modern(x: Option<u8>) { if let Some(x) = x && x > 0 { } }",
        ),
        (
            "captures",
            "pub fn modern<'a>(x: &'a u8) -> impl Copy + use<'a> { x }",
        ),
        (
            "unsafe_extern",
            "unsafe extern \"C\" { pub safe fn abs(x: i32) -> i32; }",
        ),
    ] {
        fs::write(
            root.path().join(format!("{name}.rs")),
            format!("pub fn before() {{}}\n{modern}\npub fn after() {{}}\n"),
        )
        .unwrap();
    }
    let source = FileSource::new(root.path()).unwrap();
    let mut index = Index::open("fixture", &database).unwrap();
    index.refresh(&source, None).unwrap();
    assert!(index.report().stale.is_empty());
    assert!(
        index
            .report()
            .diagnostics
            .values()
            .all(|status| matches!(status, ParseStatus::Partial { .. }))
    );
    for name in ["chains", "captures", "unsafe_extern"] {
        let answer = index
            .answer(
                &source,
                &Request::new(Verb::Explain, format!("s:{name}.rs#after")),
            )
            .unwrap();
        assert_eq!(answer.records.len(), 1);
        if index
            .report()
            .diagnostics
            .contains_key(&format!("{name}.rs"))
        {
            assert!(answer.records[0].sum.starts_with("low-precision:"));
        }
    }
}
