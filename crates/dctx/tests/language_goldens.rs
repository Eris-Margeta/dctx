// SPDX-License-Identifier: Apache-2.0
//! Registry-complete skeleton and symbol-read contract over original source fixtures.
//! Goldens contain engine records and exact source bytes, with no transport rewriting.
//! Run with DCTX_UPDATE_LANGUAGE_GOLDENS=1 to regenerate reviewed expectations.

#![forbid(unsafe_code)]
#![cfg(all(feature = "map", feature = "syntax-rust"))]

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use dctx::languages::{Confidence, LANGUAGES};
use dctx::memory::MemorySource;
use dctx::{AnswerRecord, MapRequest, SnapshotRequest, VecSink, Verb};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/languages")
}

fn fixtures(tag: &str) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(corpus().join(tag))
        .unwrap_or_else(|error| panic!("missing corpus for {tag}: {error}"))
        .map(|entry| entry.expect("fixture entry").path())
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "empty corpus for {tag}");
    paths
}

/// Only fields whose meaning is independent of encoding and trailing blank lines.
fn declarations(records: &[AnswerRecord]) -> Vec<(&str, &str, &str, &str, &str)> {
    records
        .iter()
        .filter(|record| record.kind != "module")
        .map(|record| {
            (
                record.id.as_str(),
                record.kind.as_str(),
                record.signature.as_deref().unwrap_or_default(),
                record.summary.as_str(),
                record.summary_source.as_str(),
            )
        })
        .collect()
}

fn with_long_line(tag: &str, path: &str, text: &str) -> String {
    let padding = format!("café {}", "x".repeat(4096));
    let (open, close) = match tag {
        "config" if path.ends_with(".json") => {
            // Legal JSON whitespace on a nonempty line, without adding a key.
            return text.replacen('{', &format!("{{{}", " ".repeat(4096)), 1);
        }
        "config" if !path.ends_with(".jsonc") => ("#", ""),
        "python" | "ruby" | "perl" | "powershell" | "elixir" | "r" | "julia" | "hcl" | "shell"
        | "make" | "docker" | "graphql" | "nix" => ("#", ""),
        "lua" | "haskell" | "sql" => ("--", ""),
        "erlang" | "tex" => ("%", ""),
        "fortran" => ("!", ""),
        "clojure" | "lisp" | "scheme" | "elisp" => (";", ""),
        "ocaml" => ("(*", "*)"),
        "web" => ("<!--", "-->"),
        "markdown" | "text" | "rst" | "asciidoc" | "org" => ("Long line: ", ""),
        _ => ("//", ""),
    };
    format!("{text}{open}{padding}{close}\n")
}

fn exercise(path: &str, text: &str, guess: bool) -> (Vec<AnswerRecord>, String) {
    let mut source = MemorySource::new();
    source
        .insert_file(path, text.as_bytes(), 0)
        .expect("source");
    let mut session = common::open_session(source);
    let handle = session
        .snapshot(&SnapshotRequest::immutable())
        .unwrap_or_else(|error| panic!("snapshot {path}: {error}"));
    let before = session.content_reads();
    let mut request = MapRequest::new(Verb::Skeleton);
    request.target = format!("m:{path}");
    request.limits.encoded_bytes = Some(1024 * 1024);
    let page = session
        .query(&handle, &request, &mut VecSink::new())
        .unwrap_or_else(|error| panic!("skeleton {path}: {error}"));
    assert!(page.outcome.is_none(), "{path}: {:?}", page.outcome);
    assert!(page.next.is_none(), "unexpected pagination: {path}");
    assert_eq!(session.content_reads(), before, "skeleton reread {path}");
    assert!(page.records.len() > 1, "no declarations: {path}");
    assert_eq!(page.records[0].id, format!("m:{path}"));
    assert_eq!(page.records[0].kind, "module");
    let mut golden = String::new();
    for record in &page.records {
        writeln!(
            golden,
            "skeleton {:?} {:?} {:?} signature={:?} summary={:?} source={:?} confidence={:?}",
            record.id,
            record.kind,
            record.lines,
            record.signature,
            record.summary,
            record.summary_source,
            record.confidence,
        )
        .expect("format");
        if record.kind == "module" {
            continue;
        }
        assert_eq!(
            record.confidence.as_deref(),
            guess.then_some("guess"),
            "confidence {path}: {}",
            record.id
        );
        let mut read = MapRequest::new(Verb::Read);
        read.target.clone_from(&record.id);
        read.limits.encoded_bytes = Some(1024 * 1024);
        let answer = session
            .query(&handle, &read, &mut VecSink::new())
            .unwrap_or_else(|error| panic!("read {}: {error}", record.id));
        assert!(answer.outcome.is_none(), "{}", record.id);
        assert!(answer.next.is_none(), "unexpected read cut: {}", record.id);
        assert_eq!(answer.records.len(), 1, "{}", record.id);
        let actual = &answer.records[0];
        assert_eq!(actual.id, record.id);
        assert_eq!(actual.lines, record.lines);
        let [start, end] = actual.span.expect("read span");
        let start = usize::try_from(start).expect("start");
        let end = usize::try_from(end).expect("end");
        let bytes = text.get(start..end).expect("bounded UTF-8 span");
        assert!(!bytes.is_empty(), "empty read: {}", record.id);
        assert_eq!(actual.source_utf8.as_deref(), Some(bytes), "{}", record.id);
        let line_at = |offset: usize| {
            u32::try_from(
                text.as_bytes()[..offset]
                    .iter()
                    .filter(|&&b| b == b'\n')
                    .count(),
            )
            .expect("line count")
                + 1
        };
        assert_eq!(
            [line_at(start), line_at(end - 1)],
            actual.lines,
            "span/lines disagree: {}",
            record.id
        );
        writeln!(golden, "read {:?} {:?}", actual.span, actual.source_utf8).expect("format");
    }
    (page.records, golden)
}

#[test]
fn corpus_covers_exactly_the_language_registry() {
    let dirs: BTreeSet<_> = std::fs::read_dir(corpus())
        .expect("corpus")
        .filter_map(|entry| {
            let entry = entry.expect("entry");
            entry
                .file_type()
                .expect("type")
                .is_dir()
                .then(|| entry.file_name().into_string().expect("UTF-8 tag"))
        })
        .collect();
    let tags: BTreeSet<_> = LANGUAGES
        .iter()
        .map(|language| language.tag.to_string())
        .collect();
    assert_eq!(
        dirs, tags,
        "add fixtures whenever the code registry changes"
    );
    for language in LANGUAGES {
        for fixture in fixtures(language.tag) {
            let file = fixture.file_name().expect("name").to_str().expect("UTF-8");
            let extension = fixture.extension().and_then(|ext| ext.to_str());
            assert!(
                extension.is_some_and(|ext| language.extensions.contains(&ext))
                    || language.files.contains(&file),
                "fixture {file} is not selected by {}",
                language.tag
            );
        }
    }
}

#[test]
fn skeleton_and_every_symbol_read_match_per_language_goldens() {
    let update = std::env::var_os("DCTX_UPDATE_LANGUAGE_GOLDENS").is_some();
    let mut failed = Vec::new();
    for language in LANGUAGES {
        for fixture in fixtures(language.tag) {
            let result = std::panic::catch_unwind(|| {
                let path = fixture.file_name().expect("file").to_str().expect("UTF-8");
                let text = std::fs::read_to_string(&fixture).expect("fixture source");
                assert!(!text.contains('\r') && !text.starts_with('\u{feff}'));
                let guess = language.confidence == Confidence::Guess;
                let (canonical, golden) = exercise(path, &text, guess);
                let expected = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/goldens/languages")
                    .join(language.tag)
                    .join(format!("{path}.golden"));
                if update {
                    std::fs::create_dir_all(expected.parent().expect("parent"))
                        .expect("golden dir");
                    std::fs::write(&expected, &golden).expect("write golden");
                }
                assert_eq!(
                    std::fs::read_to_string(&expected)
                        .unwrap_or_else(|error| panic!("{}: {error}", expected.display())),
                    golden,
                    "{}: review extraction before regenerating goldens",
                    fixture.display()
                );
                // Preserve the original physical bytes. Every read is independently checked
                // against its byte span; the declaration identity and signature must survive.
                let crlf = text.replace('\n', "\r\n");
                for (encoding, variant) in [
                    ("CRLF", crlf.clone()),
                    ("BOM", format!("\u{feff}{text}")),
                    ("BOM+CRLF", format!("\u{feff}{crlf}")),
                    ("long line", with_long_line(language.tag, path, &text)),
                ] {
                    let (actual, _) = exercise(path, &variant, guess);
                    assert_eq!(
                        declarations(&canonical),
                        declarations(&actual),
                        "{} ({encoding})",
                        fixture.display()
                    );
                    if encoding != "long line" {
                        let lines = |records: &[AnswerRecord]| {
                            records
                                .iter()
                                .map(|record| record.lines)
                                .collect::<Vec<_>>()
                        };
                        assert_eq!(lines(&canonical), lines(&actual), "{path} ({encoding})");
                    }
                }
            });
            if result.is_err() {
                failed.push(fixture);
            }
        }
    }
    assert!(failed.is_empty(), "failed fixtures: {failed:?}");
}

#[test]
fn focused_languages_keep_nested_generic_functions_and_unicode_names() {
    for (tag, path, outer, nested, unicode) in [
        (
            "kotlin",
            "sample.kt",
            "Boxed::copy",
            "Boxed::copy::nested",
            "Boxed::café",
        ),
        (
            "swift",
            "sample.swift",
            "Boxed::copy",
            "Boxed::copy::nested",
            "Boxed::café",
        ),
        (
            "csharp",
            "sample.cs",
            "Sample::Boxed::Copy",
            "Sample::Boxed::Copy::Nested",
            "Sample::Boxed::café",
        ),
    ] {
        let text = std::fs::read_to_string(corpus().join(tag).join(path)).expect("source");
        let (records, _) = exercise(path, &text, false);
        for name in [outer, nested, unicode] {
            let id = format!("s:{path}#{name}");
            assert!(records.iter().any(|record| record.id == id), "missing {id}");
        }
    }
}
