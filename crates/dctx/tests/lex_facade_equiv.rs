//! Map, find and explain stay aligned for a generated nested JSON file.
//! Spec: SPEC-MAP-MODE 5–6.2. The pre-change line scan is the span oracle.
//! Depends on the session helper and an in-memory source. No clock assertions.
//! `cargo nextest -p dctx` runs this. Kinds and summaries are compared record by record.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use dctx::memory::MemorySource;
use dctx::{MapRequest, SnapshotRequest, VecSink, Verb};

fn nested(depth: usize, branch: usize) -> String {
    fn write(out: &mut String, depth: usize, branch: usize, next: &mut u32) {
        out.push('{');
        for index in 0..branch {
            if index > 0 {
                out.push(',');
            }
            let id = *next;
            *next += 1;
            out.push('"');
            out.push('k');
            out.push_str(&id.to_string());
            out.push('"');
            out.push(':');
            if depth == 0 {
                out.push_str(&id.to_string());
            } else {
                write(out, depth - 1, branch, next);
            }
        }
        out.push('}');
    }
    let mut out = String::new();
    write(&mut out, depth, branch, &mut 0);
    out
}

fn line_span(text: &str, lines: [u32; 2]) -> [u64; 2] {
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
fn nested_json_map_find_and_explain_share_kinds_summaries_and_spans() {
    let text = nested(2, 5);
    let mut source = MemorySource::new();
    source.insert_file("nested.json", text.as_bytes(), 0).expect("insert");
    let mut session = common::open_session(source);
    let handle = session.snapshot(&SnapshotRequest::immutable()).expect("snapshot");
    let mut map_request = MapRequest::new(Verb::Map);
    map_request.target = "m:nested.json".to_string();
    map_request.limits.estimated_tokens = Some(2_000);
    map_request.limits.records = Some(60);
    let outline = session.query(&handle, &map_request, &mut VecSink::new()).expect("map");
    assert!(outline.records.len() > 20, "{}", outline.records.len());
    for record in &outline.records {
        let name = record.id.split_once('#').map(|(_, name)| name).unwrap_or(record.id.as_str());
        let mut find_request = MapRequest::new(Verb::Find);
        find_request.query = name.to_string();
        find_request.within = Some("nested.json".to_string());
        let found = session.query(&handle, &find_request, &mut VecSink::new()).expect("find");
        let hit = found.records.iter().find(|item| item.id == record.id).expect("hit");
        assert_eq!(hit.kind, record.kind);
        assert_eq!(hit.summary, record.summary);
        assert_eq!(hit.summary_source, record.summary_source);
        assert_eq!(hit.lines, record.lines);
        assert!(!record.kind.is_empty());
        assert!(!record.summary_source.is_empty());
    }
    for record in outline.records.iter().step_by(outline.records.len() / 3) {
        let explained = session.explain(&handle, &record.id, &mut VecSink::new()).expect("explain");
        assert_eq!(explained.records.len(), 1);
        assert_eq!(explained.records[0].summary, record.summary);
        assert_eq!(explained.records[0].kind, record.kind);
        let mut read_request = MapRequest::new(Verb::Read);
        read_request.target = record.id.clone();
        read_request.lines = Some(400);
        let read = session.query(&handle, &read_request, &mut VecSink::new()).expect("read");
        let span = read.records[0].span.expect("span");
        assert_eq!(span, line_span(&text, record.lines));
    }
}
