// SPDX-License-Identifier: Apache-2.0
//! Hostile targets and ordering properties at the public map query boundary.

use dctx_map_query::{Error, FileSource, Index, Request, Verb};
use std::fs;

fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, FileSource, Index) {
    let root = tempfile::tempdir().unwrap();
    for (path, body) in files {
        fs::write(root.path().join(path), body).unwrap();
    }
    let source = FileSource::new(root.path()).unwrap();
    let index = Index::in_memory("hostile").unwrap();
    (root, source, index)
}

fn find(query: &str) -> Request {
    let mut request = Request::new(Verb::Find, query);
    request.kinds = vec!["fn".into()];
    request
}

#[test]
fn empty_target_lists_the_map_but_find_requires_a_target() {
    let (_root, source, mut index) = fixture(&[("lib.rs", "pub fn needle() {}\n")]);
    let mut outline = Request::new(Verb::Map, "");
    outline.depth = 3;
    let answer = index.query(&source, &outline).unwrap();
    assert!(answer.records.iter().any(|r| r.id.as_str() == "m:lib.rs"));

    assert!(matches!(
        index.query(&source, &find("")),
        Err(Error::Request(message)) if message.contains("requires a target")
    ));
}

#[test]
fn one_mebibyte_target_is_refused_by_query() {
    let (_root, source, mut index) = fixture(&[("lib.rs", "pub fn needle() {}\n")]);
    let oversized = Request::new(Verb::Find, "x".repeat(1024 * 1024));
    assert!(matches!(
        index.query(&source, &oversized),
        Err(Error::Request(message)) if message.contains("too long")
    ));
    assert_eq!(
        index.query(&source, &find("needle")).unwrap().records.len(),
        1
    );
}

#[test]
fn embedded_nul_is_refused_by_query() {
    let (_root, source, mut index) = fixture(&[("lib.rs", "pub fn needle() {}\n")]);
    assert!(matches!(
        index.query(&source, &find("needle\0suffix")),
        Err(Error::Request(message)) if message.contains("control characters")
    ));
    assert_eq!(
        index.query(&source, &find("needle")).unwrap().records.len(),
        1
    );
}

#[test]
fn regex_and_glob_punctuation_does_not_change_lexical_matching() {
    let (_root, source, mut index) = fixture(&[(
        "lib.rs",
        "pub fn needle() {}\npub fn needlework() {}\npub fn unrelated() {}\n",
    )]);
    let plain = index.query(&source, &find("needle")).unwrap().records;
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].id.as_str(), "s:lib.rs#needle");
    for target in [
        r"^needle$|.*",
        "[needle]*?",
        r"\(needle\){1}",
        r#""needle" AND *"#,
    ] {
        let records = index.answer(&source, &find(target)).unwrap().records;
        let ids: Vec<_> = records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["s:lib.rs#needle"], "{target:?}");
    }
    for target in [r"[.*?^$+(){}|\]", "need*", "needl?"] {
        assert!(
            index
                .answer(&source, &find(target))
                .unwrap()
                .records
                .is_empty(),
            "{target:?}"
        );
    }
}

#[test]
fn composed_and_decomposed_accents_find_the_same_symbol() {
    let (_root, source, mut index) =
        fixture(&[("lib.rs", "pub fn café() {}\npub fn cafeteria() {}\n")]);
    index.refresh(&source, None).unwrap();
    for target in ["café", "cafe\u{301}"] {
        let answer = index.answer(&source, &find(target)).unwrap();
        assert_eq!(answer.records.len(), 1, "{target:?}");
        assert_eq!(answer.records[0].id.as_str(), "s:lib.rs#café", "{target:?}");
    }
}

#[test]
fn reserved_words_can_name_symbols() {
    let (_root, source, mut index) = fixture(&[(
        "lib.rs",
        "pub fn r#match() {}\npub fn not() {}\npub fn near() {}\npub fn or() {}\n",
    )]);
    index.refresh(&source, None).unwrap();
    for target in ["match", "NOT", "NEAR"] {
        let answer = index.answer(&source, &find(target)).unwrap();
        assert_eq!(answer.records.len(), 1, "{target}");
        assert_eq!(
            answer.records[0].id.as_str(),
            format!("s:lib.rs#{}", target.to_lowercase())
        );
    }
    // "or" is a find stop word, but still addresses the symbol by ID.
    let exact = Request::new(Verb::Explain, "s:lib.rs#or");
    let answer = index.answer(&source, &exact).unwrap();
    assert_eq!(answer.records.len(), 1);
    assert_eq!(answer.records[0].id.as_str(), "s:lib.rs#or");
}

#[test]
fn tied_find_results_and_pages_stay_in_order_across_twenty_indexes() {
    let mut files = [
        ("delta.rs", "pub fn needle() {}\n"),
        ("alpha.rs", "pub fn needle() {}\n"),
        ("charlie.rs", "pub fn needle() {}\n"),
        ("bravo.rs", "pub fn needle() {}\n"),
    ];
    let expected = [
        "s:alpha.rs#needle",
        "s:bravo.rs#needle",
        "s:charlie.rs#needle",
        "s:delta.rs#needle",
    ];
    for run in 0..20 {
        files.rotate_left(1);
        if run % files.len() == 0 {
            files.swap(0, 1);
        }
        let (_root, source, mut index) = fixture(&files);
        let mut request = find("needle");
        let answer = index.query(&source, &request).unwrap();
        let ids: Vec<_> = answer.records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, expected, "run {run}");
        assert!(
            answer
                .records
                .windows(2)
                .all(|pair| pair[0].score == pair[1].score)
        );

        request.limit = 2;
        for offset in [0, 2] {
            request.offset = offset;
            let page = index.answer(&source, &request).unwrap();
            let ids: Vec<_> = page.records.iter().map(|r| r.id.as_str()).collect();
            assert_eq!(
                ids,
                expected[offset..offset + 2],
                "run {run}, offset {offset}"
            );
            assert_eq!(page.more, 2 - offset);
        }
    }
}
