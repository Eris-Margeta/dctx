// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile requests, source bytes, cached JSON and render inputs.
use dctx_map_query::{
    Answer, AnswerBudget, Direction, EdgeKind, FileSource, Format, Index, ParseStatus, Request,
    Verb,
    ports::{FileStamp, Snapshot, Source, SourceFile},
    render_answer, render_answer_at,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
struct MemorySource(BTreeMap<String, Vec<u8>>);

impl MemorySource {
    fn one(bytes: &[u8]) -> Self {
        Self(BTreeMap::from([("lib.rs".into(), bytes.to_vec())]))
    }
}

impl Source for MemorySource {
    fn snapshot(&self) -> dctx_map_query::Result<Snapshot> {
        let files = self
            .0
            .iter()
            .map(|(path, bytes)| {
                let digest = dctx_hash::Sha256::digest(bytes).unwrap();
                let hash = digest
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                (
                    path.clone(),
                    SourceFile {
                        bytes: Some(bytes.clone()),
                        hash,
                        size: bytes.len() as u64,
                        mtime: 0,
                        readable: true,
                        allowed: true,
                        stamp: FileStamp::default(),
                    },
                )
            })
            .collect();
        Ok(Snapshot {
            files,
            revision: "abcdef0123456789".into(),
            dirty: BTreeSet::new(),
            recent: BTreeSet::new(),
            directories: BTreeMap::new(),
            rules: BTreeMap::new(),
        })
    }
    fn read(&self, path: &str) -> dctx_map_query::Result<Vec<u8>> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| dctx_map_query::Error::Denied(path.into()))
    }
}

fn fixture() -> (MemorySource, Index) {
    let source = MemorySource::one(b"pub fn a() { b(); }\npub fn b() {}\n");
    let mut index = Index::in_memory("hostile").unwrap();
    index.refresh(&source, None).unwrap();
    (source, index)
}

fn empty_answer(verb: Verb) -> Answer {
    Answer {
        place: "hostile".into(),
        rev: "abcdef0".into(),
        dirty: 0,
        age_seconds: 0,
        verb,
        records: vec![],
        more: 0,
        next: "dctx map map".into(),
        identity: None,
        reason: Some("no match; try dctx map map".into()),
    }
}

#[test]
fn every_verb_covers_empty_one_byte_and_target_byte_limits() {
    for verb in [
        Verb::Map,
        Verb::Find,
        Verb::Neighbors,
        Verb::Explain,
        Verb::Read,
    ] {
        for len in [0, 1, 4096, 4097] {
            let request = Request::new(verb, "x".repeat(len));
            let valid = (verb == Verb::Map || len != 0) && len <= 4096;
            assert_eq!(request.validate().is_ok(), valid, "{verb:?}/{len}");
            assert_eq!(request.validate_as("alias").is_ok(), valid);
        }
        for text in ["\0", "a\nb", "a\rb", "a\tb"] {
            assert!(Request::new(verb, text).validate().is_err());
        }
        // The target limit counts UTF-8 bytes, not Unicode scalar values.
        assert!(Request::new(verb, "é".repeat(2048)).validate().is_ok());
        assert!(Request::new(verb, "é".repeat(2049)).validate().is_err());
    }
}

#[test]
fn every_verb_enforces_numeric_bounds_without_overflow() {
    for verb in [
        Verb::Map,
        Verb::Find,
        Verb::Neighbors,
        Verb::Explain,
        Verb::Read,
    ] {
        let mut r = Request::new(verb, "x");
        let max_limit = if verb == Verb::Find { 40 } else { 60 };
        let max_depth = if verb == Verb::Neighbors { 4 } else { 3 };
        for limit in [0, 1, max_limit, max_limit + 1, usize::MAX] {
            r.limit = limit;
            assert_eq!(r.validate().is_ok(), (1..=max_limit).contains(&limit));
        }
        r.limit = 1;
        for depth in [0, 1, max_depth, max_depth + 1, u8::MAX] {
            r.depth = depth;
            assert_eq!(r.validate().is_ok(), (1..=max_depth).contains(&depth));
        }
    }
    for full in [false, true] {
        let max = if full { 400 } else { 120 };
        for (lines, valid) in [
            ([0, 1], false),
            ([2, 1], false),
            ([1, max], true),
            ([1, max + 1], false),
            ([u32::MAX, u32::MAX], true),
            ([1, u32::MAX], false),
        ] {
            let mut r = Request::new(Verb::Read, "s:lib.rs#a");
            r.full = full;
            r.lines = Some(lines);
            assert_eq!(r.validate().is_ok(), valid, "{lines:?}/{full}");
        }
    }
}

#[test]
fn paths_and_request_prefixes_refuse_escapes_and_controls() {
    for path in [
        "", "..", "../x", "a/../x", "/x", "a\\b", "a:b", "a//b", "a\0b",
    ] {
        let mut r = Request::new(Verb::Find, "a");
        r.within = Some(path.into());
        assert!(r.validate().is_err(), "{path:?}");
        r.within = None;
        r.focus = vec![path.into()];
        assert!(r.validate().is_err());
    }
    let sample = "s:lib.rs#café";
    for cut in 0..=sample.len() {
        if let Ok(text) = std::str::from_utf8(&sample.as_bytes()[..cut]) {
            for verb in [
                Verb::Map,
                Verb::Find,
                Verb::Neighbors,
                Verb::Explain,
                Verb::Read,
            ] {
                let _ = Request::new(verb, text).validate();
            }
        }
    }
    let invalid = vec![0xff];
    assert!(std::str::from_utf8(&invalid).is_err()); // `str` APIs cannot receive invalid UTF-8.
}

#[test]
fn every_source_prefix_and_invalid_utf8_can_be_indexed_without_panics() {
    let sample = "pub fn café() { missing(); }\n".as_bytes();
    let mut corpus: Vec<_> = (0..=sample.len())
        .map(|cut| sample[..cut].to_vec())
        .collect();
    corpus.extend([
        vec![0xff],
        vec![0xc0, 0xaf],
        vec![0, 0],
        b"pub fn a(\xff".to_vec(),
    ]);
    for bytes in corpus {
        let source = MemorySource::one(&bytes);
        let mut index = Index::in_memory("hostile").unwrap();
        index.refresh(&source, None).unwrap();
        assert!(index.report().stale.is_empty());
        let r = Request::new(Verb::Map, "");
        let expected =
            render_answer(&index.answer(&source, &r).unwrap(), &r, Format::Json).unwrap();
        index.refresh(&source, None).unwrap();
        assert_eq!(index.report().parsed, 0);
        assert_eq!(
            render_answer(&index.answer(&source, &r).unwrap(), &r, Format::Json)
                .unwrap()
                .as_bytes(),
            expected.as_bytes()
        );
        if std::str::from_utf8(&bytes).is_err() {
            assert!(matches!(
                index.report().diagnostics.get("lib.rs"),
                Some(ParseStatus::Unparseable { .. })
            ));
            if let Ok(read) = index.answer(&source, &Request::new(Verb::Read, "m:lib.rs")) {
                assert!(read.records.is_empty(), "invalid UTF-8 must not be served");
            }
        }
    }
}

#[test]
fn empty_inventory_and_all_five_verbs_have_stable_outputs() {
    for source in [
        MemorySource::default(),
        MemorySource::one(b""),
        MemorySource::one(b"x"),
    ] {
        let mut index = Index::in_memory("hostile").unwrap();
        index.refresh(&source, None).unwrap();
        for (verb, target) in [
            (Verb::Map, ""),
            (Verb::Find, "x"),
            (Verb::Neighbors, "s:lib.rs#a"),
            (Verb::Explain, "s:lib.rs#a"),
            (Verb::Read, "s:lib.rs#a"),
        ] {
            let r = Request::new(verb, target);
            for format in [Format::Text, Format::Json] {
                let first = render_answer(&index.answer(&source, &r).unwrap(), &r, format).unwrap();
                assert_eq!(
                    render_answer(&index.answer(&source, &r).unwrap(), &r, format)
                        .unwrap()
                        .as_bytes(),
                    first.as_bytes()
                );
                assert!(r.budget.fits(&first));
            }
        }
    }
}

#[test]
fn duplicate_reordered_filters_do_not_duplicate_query_results() {
    let (source, index) = fixture();
    let mut request = Request::new(Verb::Neighbors, "s:lib.rs#a");
    request.direction = Direction::Out;
    request.edges = vec![EdgeKind::Calls, EdgeKind::Imports];
    let expected = index.answer(&source, &request).unwrap();
    request.edges = vec![EdgeKind::Imports, EdgeKind::Calls, EdgeKind::Calls];
    assert_eq!(index.answer(&source, &request).unwrap(), expected);
    let mut request = Request::new(Verb::Find, "b");
    request.kinds = vec!["fn".into(), "module".into()];
    request.focus = vec!["lib.rs".into(), ".".into()];
    let expected = index.answer(&source, &request).unwrap();
    request.kinds.reverse();
    request.kinds.push("fn".into());
    request.focus.reverse();
    request.focus.push("lib.rs".into());
    assert_eq!(index.answer(&source, &request).unwrap(), expected);
}

#[test]
fn maximum_offset_and_remaining_count_never_panic_or_wrap() {
    let (source, index) = fixture();
    let mut r = Request::new(Verb::Find, "b");
    r.offset = usize::MAX;
    let answer = index.answer(&source, &r).unwrap();
    assert!(answer.records.is_empty());
    assert_eq!(answer.more, 0);
    r.offset = 0;
    let mut answer = index.answer(&source, &r).unwrap();
    assert!(!answer.records.is_empty());
    answer.more = usize::MAX;
    for format in [Format::Text, Format::Json] {
        let result = render_answer(&answer, &r, format);
        // Either a representable full page or a typed count-overflow refusal is valid.
        if let Ok(output) = result {
            assert!(output.contains(&usize::MAX.to_string()));
        }
    }
}

#[test]
fn source_pagination_handles_maximum_line_numbers_and_malformed_spans() {
    let (source, index) = fixture();
    let mut r = Request::new(Verb::Read, "s:lib.rs#a");
    r.budget = AnswerBudget::new(300).unwrap();
    let mut answer = index.answer(&source, &r).unwrap();
    answer.records[0].l = [u32::MAX - 120, u32::MAX];
    answer.records[0].body = Some("long source line with repeated words\n".repeat(121));
    for format in [Format::Text, Format::Json] {
        let result = render_answer(&answer, &r, format).unwrap();
        assert!(r.budget.fits(&result));
        assert_eq!(render_answer(&answer, &r, format).unwrap(), result);
    }
    for l in [[u32::MAX, u32::MAX], [0, 0], [2, 1]] {
        answer.records[0].l = l;
        assert!(render_answer(&answer, &r, Format::Json).is_err());
    }
}

#[test]
fn renderers_refuse_invalid_metadata_and_cover_budget_boundaries() {
    for verb in [
        Verb::Map,
        Verb::Find,
        Verb::Neighbors,
        Verb::Explain,
        Verb::Read,
    ] {
        let mut answer = empty_answer(verb);
        let mut request = Request::new(verb, "x");
        for budget in [1, 2000] {
            request.budget = AnswerBudget::new(budget).unwrap();
            for format in [Format::Text, Format::Json] {
                let output =
                    render_answer_at(&answer, &request, format, Some("place with ' quote"));
                assert_eq!(output.is_ok(), budget == 2000);
                if let Ok(output) = output {
                    assert!(request.budget.fits(&output));
                }
            }
        }
        for revision in ["", "a", "abcdef", "abcdefg", "abc\n012"] {
            answer.rev = revision.into();
            assert!(render_answer(&answer, &request, Format::Json).is_err());
        }
        assert!(AnswerBudget::new(0).is_err());
        assert!(AnswerBudget::new(2001).is_err());
    }
}

#[test]
fn renderers_allow_concurrent_shared_answers_with_byte_identical_results() {
    let (source, index) = fixture();
    let request = Request::new(Verb::Explain, "s:lib.rs#a");
    let answer = index.answer(&source, &request).unwrap();
    for format in [Format::Text, Format::Json] {
        let expected = render_answer(&answer, &request, format).unwrap();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| render_answer(&answer, &request, format).unwrap()))
                .collect();
            for h in handles {
                assert_eq!(h.join().unwrap().as_bytes(), expected.as_bytes());
            }
        });
    }
    // Index and FileSource contain non-Sync adapters; each thread owns its own instance.
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let (source, mut index) = fixture();
                    let r = Request::new(Verb::Find, "b");
                    render_answer(&index.query(&source, &r).unwrap(), &r, Format::Json).unwrap()
                })
            })
            .collect();
        let values: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(
            values
                .windows(2)
                .all(|w| w[0].as_bytes() == w[1].as_bytes())
        );
    });
}

#[test]
fn persistent_payload_prefixes_reordering_and_repeats_preserve_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("index.sqlite");
    let source = MemorySource::one(b"pub fn a() {}\n");
    let mut index = Index::open("hostile", &database).unwrap();
    index.refresh(&source, None).unwrap();
    let payload = index.file_payload("lib.rs").unwrap();
    for end in 0..payload.len() {
        if let Ok(prefix) = std::str::from_utf8(&payload.as_bytes()[..end]) {
            assert!(
                index.replace_file_payload("lib.rs", prefix).is_err(),
                "cut {end}"
            );
            assert_eq!(index.file_payload("lib.rs").unwrap(), payload);
        }
    }
    for bad in [
        "",
        "x",
        "{}",
        "[]",
        "null",
        "{\"hash\":\"a\",\"hash\":\"b\"}",
    ] {
        assert!(index.replace_file_payload("lib.rs", bad).is_err());
    }
    let mut value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    value["nodes"].as_array_mut().unwrap().reverse();
    index
        .replace_file_payload("lib.rs", &value.to_string())
        .unwrap();
    index.refresh(&source, None).unwrap();
    assert!(index.stores_agree().unwrap());
    index.save(&database).unwrap();
    assert!(index.save(dir.path().join("other.sqlite")).is_err());
    assert!(index.file_payload("").is_err());
    index.mark_publication_interrupted().unwrap();
    drop(index);
    let mut reopened = Index::open("hostile", &database).unwrap();
    reopened.refresh(&source, None).unwrap();
    assert!(reopened.stores_agree().unwrap());
}

#[test]
fn cancellation_and_unready_entry_points_refuse_with_no_io() {
    let source = MemorySource::default();
    let mut index = Index::in_memory("hostile").unwrap();
    let request = Request::new(Verb::Map, "");
    assert!(index.answer(&source, &request).is_err());
    assert!(index.replace_file_payload("", "{}").is_err());
    assert!(index.mark_publication_interrupted().is_err());
    assert!(index.overlay().added.is_empty());
    index.cancel();
    index.cancel();
    assert!(index.refresh(&source, None).is_err());
    assert!(index.query(&source, &request).is_err());
}

#[test]
fn filesystem_source_entry_points_recheck_paths_and_preserve_exact_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let sample = b"pub fn a() {}\xff";
    std::fs::write(dir.path().join("a.rs"), sample).unwrap();
    assert!(FileSource::new(dir.path().join("a.rs")).is_err());
    assert!(FileSource::strict(dir.path().join("missing")).is_err());
    for source in [
        FileSource::new(dir.path()).unwrap(),
        FileSource::strict(dir.path()).unwrap(),
    ] {
        assert_eq!(source.root(), dir.path());
        for bad in ["", "../x", "/x", "a\\b", "a:b", "a\0b", ".git/HEAD"] {
            assert!(source.read(bad).is_err(), "{bad:?}");
        }
        let snapshot = source.snapshot().unwrap();
        assert_eq!(source.read("a.rs").unwrap(), sample);
        assert!(source.unchanged("a.rs", &snapshot.files["a.rs"]).unwrap());
        let again = source.snapshot_since(Some(&snapshot)).unwrap();
        assert_eq!(snapshot.revision, again.revision);
        assert!(source.content_reads() > 0);
        assert!(!source.opened_paths().is_empty());
    }
}

#[test]
fn query_refusals_name_a_next_step_and_preserve_error_sources() {
    let source = MemorySource::default();
    let mut index = Index::in_memory("hostile").unwrap();
    let mut bad = Request::new(Verb::Find, "");
    let mut errors = vec![bad.validate().unwrap_err()];
    bad.target = "a".into();
    bad.limit = usize::MAX;
    errors.push(bad.validate_as("grep").unwrap_err());
    errors.push(
        index
            .answer(&source, &Request::new(Verb::Map, ""))
            .unwrap_err(),
    );
    errors.push(dctx_map_query::Error::Io(std::io::Error::from(
        std::io::ErrorKind::PermissionDenied,
    )));
    errors.push(dctx_map_query::Error::Json(
        serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
    ));
    errors.push(dctx_map_query::Error::Denied("private".into()));
    index.cancel();
    errors.push(index.refresh(&source, None).unwrap_err());
    for error in errors {
        assert!(error.to_string().contains("; next: "), "{error}");
    }
    let error = dctx_map_query::Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
    assert!(std::error::Error::source(&error).is_some());
}
