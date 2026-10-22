//! Query records keep pre-index byte spans for JSON line shapes.
//! Implements DCTX-COLD-SCAN C1: compact, pretty, CRLF, UTF-8, and no final newline.
//! Depends on the public Session and an in-memory source. The oracle is the old scan.
//! `cargo nextest -p dctx` runs this. Read records carry the span; nothing times the lookup.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use dctx::memory::MemorySource;
use dctx::{MapRequest, SnapshotRequest, VecSink, Verb};

const FIXTURES: &[(&str, &str)] = &[
    ("compact.json", "{\"Alpha\":1,\"Beta\":{\"Gamma\":2},\"Delta\":[3]}"),
    ("pretty.json", "{\n  \"Alpha\": 1,\n\n  \"Beta\": {\n    \"Gamma\": 2\n  }\n}\n"),
    ("crlf.json", "{\r\n  \"Alpha\": 1,\r\n\r\n  \"Beta\": 2\r\n}\r\n"),
    ("multi.json", "{\n  \"名前\": \"café\",\n  \"Key\": \"🔑\"\n}\n"),
    ("tail.json", "{\n  \"Alpha\": 1,\n\n  \"Tail\": \"é\"}"),
];

/// Byte-for-byte the pre-index `line_span` scan.
fn pre_index_span(text: &str, lines: [u32; 2]) -> [u64; 2] {
    let start_line = lines[0].max(1);
    let end_line = lines[1].max(start_line);
    let mut offset = 0usize;
    let mut start = 0usize;
    let mut end = text.len();
    let mut seen = false;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let line_no = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
        if line_no == start_line {
            start = offset;
            seen = true;
        }
        if line_no == end_line {
            end = offset.saturating_add(line.trim_end_matches(['\r', '\n']).len());
            break;
        }
        offset = offset.saturating_add(line.len());
    }
    if !seen {
        return [0, 0];
    }
    [u64::try_from(start).unwrap_or(u64::MAX), u64::try_from(end).unwrap_or(u64::MAX)]
}

#[test]
fn query_records_match_the_pre_index_spans() {
    let mut source = MemorySource::new();
    for (path, text) in FIXTURES {
        source.insert_file(path, text.as_bytes(), 0).expect(path);
    }
    let mut session = common::open_session(source);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");

    for (path, text) in FIXTURES {
        let outline = session.map(&handle, &format!("m:{path}"), &mut VecSink::new()).expect(path);
        assert!(!outline.records.is_empty(), "{path} produced no symbols");
        assert_eq!(outline.more, 0, "{path}");
        for record in &outline.records {
            let name = record.id.split_once('#').map_or_else(|| panic!("id {}", record.id), |(_, name)| name);
            let mut find_request = MapRequest::new(Verb::Find);
            find_request.query = name.to_string();
            find_request.within = Some((*path).to_string());
            let found = session.query(&handle, &find_request, &mut VecSink::new()).unwrap_or_else(|error| panic!("{path} find: {error}"));
            let hit = found.records.iter().find(|item| item.id == record.id).unwrap_or_else(|| panic!("{path} find missed {name}"));
            assert_eq!(hit.kind, record.kind, "{path} {name}");
            assert_eq!(hit.path, record.path, "{path} {name}");
            assert_eq!(hit.lines, record.lines, "{path} {name}");
            assert_eq!(hit.summary, record.summary, "{path} {name}");
            assert_eq!(hit.summary_source, record.summary_source, "{path} {name}");
            assert_eq!(hit.span, None, "{path} {name}");

            let explained = session.explain(&handle, &record.id, &mut VecSink::new()).unwrap_or_else(|error| panic!("{path} explain: {error}"));
            assert_eq!(explained.records.len(), 1, "{path} {name}");
            assert_eq!(explained.records[0].lines, record.lines, "{path} {name}");
            assert_eq!(explained.records[0].summary, record.summary, "{path} {name}");
            assert_eq!(explained.records[0].summary_source, record.summary_source, "{path} {name}");

            let mut read_request = MapRequest::new(Verb::Read);
            read_request.target = record.id.clone();
            read_request.lines = Some(400);
            let read = session.query(&handle, &read_request, &mut VecSink::new()).unwrap_or_else(|error| panic!("{path} read: {error}"));
            assert_eq!(read.records.len(), 1, "{path} {name}");
            let got = &read.records[0];
            assert_eq!(got.lines, record.lines, "{path} {name}");
            assert_eq!(got.kind, record.kind, "{path} {name}");
            assert_eq!(got.path, *path, "{path} {name}");
            assert_eq!(got.summary, record.summary, "{path} {name}");
            let span = got.span.unwrap_or_else(|| panic!("{path} {name} has no span"));
            assert_eq!(span, pre_index_span(text, record.lines), "{path} {name}");
            let start = usize::try_from(span[0]).unwrap_or(usize::MAX);
            let end = usize::try_from(span[1]).unwrap_or(usize::MAX);
            assert!(text.is_char_boundary(start) && text.is_char_boundary(end), "{path} {name} {span:?}");
            assert_eq!(got.source_utf8.as_deref(), Some(&text[start..end]), "{path} {name}");
        }
    }
}
