// SPDX-License-Identifier: Apache-2.0
use super::{parse_resume, read_cursor, slice_span, truncation_line};

#[test]
fn absolute_lines_match_the_file_and_a_prefix_shift_does_not() {
    let bytes = b"L01\nL02\nL03\nL04\n";
    let span = [0, u64::try_from(bytes.len()).unwrap_or(0)];
    let exact = slice_span(bytes, span, [1, 4], 2, Some(3)).expect("range");
    assert_eq!(exact.text, "L03\nL04\n");
    assert!(!exact.capped);
    let prefix = slice_span(bytes, span, [1, 4], 2, None).expect("prefix");
    assert_eq!(prefix.text, "L01\nL02\n[DCTX_OMITTED]\n");
    assert!(prefix.capped);
    let outside = slice_span(bytes, span, [1, 4], 2, Some(4)).expect_err("past end");
    assert!(outside.to_string().contains("outside"), "{outside}");
}

#[test]
fn a_noninitial_range_inside_a_span_is_not_the_span_prefix() {
    let bytes = b"line01\nline02\nline03\nline04\nline05\n";
    let span = [7, 28];
    let inner = slice_span(bytes, span, [2, 4], 2, Some(3)).expect("inner");
    assert_eq!(inner.text, "line03\nline04\n");
    assert_ne!(inner.text, "line02\nline03\n");
    let outside = slice_span(bytes, span, [2, 4], 1, Some(1)).expect_err("before node");
    assert!(outside.to_string().contains("outside"), "{outside}");
}

#[test]
fn a_read_cursor_names_the_next_line_and_keeps_the_id() {
    use crate::domain::Verb;

    let cursor = read_cursor(4, "s:a.rs#inner", 12, 40, true);
    let id = crate::domain::parse_cursor(&cursor, 4, Verb::Read).expect("cursor");
    let resume = parse_resume(&id).expect("resume");
    assert_eq!(resume.id, "s:a.rs#inner");
    assert_eq!(resume.start, 12);
    assert_eq!(resume.end, 40);
    assert!(resume.beyond);
    assert_eq!(truncation_line(12, 40), "[DCTX_TRUNCATED] lines 12-40\n");
    assert!(parse_resume("m:a.rs").is_err());
}

proptest::proptest! {
    #[test]
    fn robustness_read_resume_and_spans(
        text in ".{0,512}", start in proptest::prelude::any::<u64>(),
        end in proptest::prelude::any::<u64>(),
    ) {
        let _ = parse_resume(&text);
        let _ = slice_span(text.as_bytes(), [start, end], [1, 1], 10, None);
    }
}
