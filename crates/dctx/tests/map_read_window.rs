// SPDX-License-Identifier: Apache-2.0
//! Read line windows and over-budget source cuts.
//! Implements contracts/API-PORTS-v1.md read ranges and `BudgetTooSmall`.
//! Depends on Session and an in-memory source. No clock and no network.
//! `cargo nextest -p dctx` runs this. Depth is not a line number.
//! A cut page names the next lines; following it yields each source line once.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
#[path = "common/mod.rs"]
mod common;

use dctx::memory::MemorySource;
use dctx::{DctxError, MapRequest, SnapshotRequest, VecSink, Verb};

use common::open_session;

fn numbered_file() -> String {
    let mut body = String::new();
    for number in 1..=40 {
        let _ = writeln!(body, "// L{number:02}");
    }
    body
}

fn wide_file() -> String {
    let mut body = String::from("fn BudgetBeacon() {\n    let kept = 1;\n}\n");
    let pad = "// budget-line ....................................\n";
    while body.len() < 4 * 1024 {
        body.push_str(pad);
    }
    body.push_str("// BUDGET_TAIL\n");
    body
}

fn without_mark(source: &str) -> String {
    let mut lines: Vec<&str> = source.split_inclusive('\n').collect();
    if lines
        .last()
        .is_some_and(|line| line.starts_with("[DCTX_TRUNCATED]") || *line == "[DCTX_OMITTED]\n")
    {
        lines.pop();
    }
    lines.concat()
}

#[test]
fn line_start_reads_disk_lines_and_a_positive_depth_is_rejected() {
    let text = numbered_file();
    let mut source = MemorySource::new();
    source
        .insert_file("lines.rs", text.as_bytes(), 0)
        .expect("lines");
    let mut session = open_session(source);
    let handle = session
        .snapshot(&SnapshotRequest::immutable())
        .expect("snapshot");

    let mut exact = MapRequest::new(Verb::Read);
    exact.target = "m:lines.rs".to_string();
    exact.line_start = Some(35);
    exact.lines = Some(2);
    assert!(exact.depth.is_none(), "depth stays unset");
    let page = session
        .query(&handle, &exact, &mut VecSink::new())
        .expect("lines");
    let got = page.records[0].source_utf8.as_deref().expect("source");
    assert_eq!(got, "// L35\n// L36\n");
    assert!(page.next.is_none(), "{page:?}");

    let mut with_depth = exact.clone();
    with_depth.depth = Some(0);
    let page = session
        .query(&handle, &with_depth, &mut VecSink::new())
        .expect("depth 0");
    assert_eq!(
        page.records[0].source_utf8.as_deref(),
        Some("// L35\n// L36\n")
    );

    let reads = session.content_reads();
    let mut smuggled = MapRequest::new(Verb::Read);
    smuggled.target = "m:lines.rs".to_string();
    smuggled.depth = Some(35);
    smuggled.lines = Some(2);
    let mut sink = VecSink::new();
    let error = session
        .query(&handle, &smuggled, &mut sink)
        .expect_err("depth");
    assert!(error.to_string().contains("depth"), "{error}");
    assert!(sink.is_empty());
    assert_eq!(session.content_reads(), reads);

    let mut both = smuggled.clone();
    both.line_start = Some(35);
    both.depth = Some(2);
    let mut sink = VecSink::new();
    let error = session
        .query(&handle, &both, &mut sink)
        .expect_err("depth and lines");
    assert!(matches!(error, DctxError::InvalidRequest { .. }), "{error}");
    assert!(error.to_string().contains("depth"), "{error}");
    assert!(sink.is_empty());
}

#[test]
fn a_module_past_the_budget_returns_a_marked_prefix_then_the_rest_once() {
    let text = wide_file();
    assert!(text.len() >= 4 * 1024, "{}", text.len());
    assert!(text.lines().count() <= 120, "{}", text.lines().count());
    let mut source = MemorySource::new();
    source
        .insert_file("wide.rs", text.as_bytes(), 0)
        .expect("wide");
    let mut session = open_session(source);
    let handle = session
        .snapshot(&SnapshotRequest::immutable())
        .expect("snapshot");

    let mut full = MapRequest::new(Verb::Read);
    full.target = "m:wide.rs".to_string();
    let mut sink = VecSink::new();
    let whole = session.query(&handle, &full, &mut sink).expect("full");
    assert_eq!(whole.records[0].source_utf8.as_deref(), Some(text.as_str()));
    assert!(whole.next.is_none());
    let full_len = u64::try_from(sink.as_bytes().len()).unwrap_or(0);

    let mut tiny = MapRequest::new(Verb::Read);
    tiny.target = "m:wide.rs".to_string();
    tiny.limits.encoded_bytes = Some(1);
    let mut refused = VecSink::new();
    let error = session
        .query(&handle, &tiny, &mut refused)
        .expect_err("signature");
    let DctxError::BudgetTooSmall { minimum_bytes } = error else {
        panic!("expected BudgetTooSmall, got {error}");
    };
    assert!(refused.is_empty());
    assert!(minimum_bytes > 1, "{minimum_bytes}");
    assert!(
        minimum_bytes < full_len,
        "needed {minimum_bytes}, full page {full_len}"
    );

    let mut request = MapRequest::new(Verb::Read);
    request.target = "m:wide.rs".to_string();
    request.limits.encoded_bytes = Some(minimum_bytes + (full_len - minimum_bytes) / 2);
    let mut seen = String::new();
    let mut pages = 0u32;
    loop {
        pages += 1;
        assert!(
            pages <= 120,
            "continuation did not finish after {pages} pages"
        );
        let mut sink = VecSink::new();
        let page = session.query(&handle, &request, &mut sink).expect("page");
        let source = page.records[0].source_utf8.as_deref().expect("source");
        let marked = source.contains("[DCTX_TRUNCATED]");
        seen.push_str(&without_mark(source));
        if let Some(token) = page.next.clone() {
            assert!(marked, "{source}");
            assert!(page.more > 0, "{page:?}");
            request.continuation = Some(token);
            request.line_start = None;
        } else {
            assert!(!marked, "{source}");
            break;
        }
    }
    assert!(pages > 1, "the module fit in one page");
    assert_eq!(seen, text);
}

#[test]
fn json_like_cut_moves_closing_lines_to_the_exact_continuation() {
    let text = format!(
        "{{\r\n  \"first\": {{\r\n    \"name\": \"café\"\r\n  }},\r\n\r\n  \"second\": \"{}\"\r\n}}\r\n",
        "x".repeat(2000)
    );
    for format in [2, 1] {
        let mut source = MemorySource::new();
        source
            .insert_file("sample.json", text.as_bytes(), 0)
            .expect("source");
        let mut session = open_session(source);
        let handle = session
            .snapshot(&SnapshotRequest::immutable())
            .expect("snapshot");
        let mut request = MapRequest::new(Verb::Read);
        request.target = "m:sample.json".to_string();
        request.output_format = format;
        request.limits.encoded_bytes = Some(900);
        let mut sink = VecSink::new();
        let page = session
            .query(&handle, &request, &mut sink)
            .expect("cut page");
        assert!(sink.as_bytes().len() <= 900);
        let body = page.records[0].source_utf8.as_deref().expect("source");
        let prefix = without_mark(body);
        assert_eq!(prefix, "{\r\n  \"first\": {\r\n    \"name\": \"café\"\r\n");
        assert_eq!(page.more, 4);
        let cursor = page.next.expect("continuation");
        assert!(cursor.contains("|4|7|"), "{cursor}");
        request.continuation = Some(cursor);
        let mut refused = VecSink::new();
        let error = session
            .query(&handle, &request, &mut refused)
            .expect_err("next content line needs a larger budget");
        assert!(matches!(error, DctxError::BudgetTooSmall { .. }), "{error}");
        assert!(
            refused.is_empty(),
            "a refusal must not emit isolated closing lines"
        );
        request.limits.encoded_bytes = None;
        let rest = session
            .query(&handle, &request, &mut VecSink::new())
            .expect("rest");
        assert_eq!(
            prefix
                + rest.records[0]
                    .source_utf8
                    .as_deref()
                    .expect("remaining source"),
            text
        );
        assert_eq!(rest.more, 0);
        assert!(rest.next.is_none());
    }
}
