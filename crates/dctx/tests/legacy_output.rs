//! Legacy layout byte parity through the public pack facade (D3-03).
//! Uses injected memory ports, fixed metadata and the D3-01 pinned snapshot bytes.
//! Covers diff selection, final byte caps and explicit SVG/configuration handling.
//! No fixture generation, filesystem writes or legacy formatter calls.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
use dctx::{DctxError, HostCeiling, HostGrants, OperationLimits, PackRequest, RuleTexts, SelectionPolicy,
    Session, Source, StoragePolicy, Timestamp, VecSink};
use dctx_template::{LegacyLayout, LegacyOutput, Template, TreeStamps};

use common::SharedSource;

const SNAPSHOT: &str = include_str!("../../dctx-template/tests/fixtures/legacy-snapshot.txt");
const TREE: &str = include_str!("../../dctx-template/tests/fixtures/legacy-tree.txt");
const SNAPSHOT_STAMPED: &str = include_str!("../../dctx-template/tests/fixtures/legacy-snapshot-stamped.txt");
const TREE_STAMPED: &str = include_str!("../../dctx-template/tests/fixtures/legacy-tree-stamped.txt");

/// Both built-in modes with their pinned bytes: the default (no `MOD:`) and the stamped 1.0.0 text.
const MODES: [(TreeStamps, &str, &str); 2] =
    [(TreeStamps::Off, SNAPSHOT, TREE), (TreeStamps::On, SNAPSHOT_STAMPED, TREE_STAMPED)];

fn request(layout: LegacyLayout) -> PackRequest {
    request_with(layout, TreeStamps::Off)
}

fn request_with(layout: LegacyLayout, stamps: TreeStamps) -> PackRequest {
    let mut template = stamps.template();
    template.insert_variable("project", "Corpus");
    let mut output = LegacyOutput::new(template, layout);
    output.debug_ignored = true;
    PackRequest { legacy_output: Some(output), ..PackRequest::new() }
}

fn session(source: impl Source + Send + 'static, cap: Option<u64>) -> Session {
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts {
            local_ignore: Some("secret.env\n.dctx/\n"),
            local_omit: Some("!*.svg\n"),
            ..RuleTexts::default()
        }))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
        .limits(OperationLimits { encoded_bytes: cap, ..OperationLimits::default() })
        .place("fixture")
        .view("pack")
        .build().expect("session")
}

fn snapshot_source() -> MemorySource {
    let mut source = MemorySource::new();
    for (path, bytes, modified) in [
        (".git/config", b"sensitive".as_slice(), 1700),
        ("Cargo.lock", b"omitted".as_slice(), 1500),
        ("logo.png", b"\x89PNG\0\x01\x02".as_slice(), 1400),
        ("secret.env", b"SECRET=1\n".as_slice(), 1300),
        ("src/main.rs", b"fn main() {}\n".as_slice(), 1200),
    ] {
        source.insert_file(path, bytes, modified).expect("file");
    }
    for (path, modified) in [("", 1000), (".git", 1600), ("src", 1100)] {
        source.insert_dir(path, modified).expect("directory");
    }
    for path in [".git/config", "Cargo.lock", "secret.env"] {
        source.fail_reads_of(path);
    }
    source
}

#[test]
fn facade_snapshot_and_tree_match_pinned_legacy_bytes() {
    for (stamps, snapshot, tree) in MODES {
        let source = SharedSource::new(snapshot_source());
        let spy = source.handle();
        let mut session = session(source, None);
        for (layout, expected) in [(LegacyLayout::Snapshot, snapshot), (LegacyLayout::TreeOnly, tree)] {
            let mut sink = VecSink::new();
            let report = session.pack(&request_with(layout, stamps), &mut sink).expect("pack");
            assert_eq!(sink.as_bytes(), expected.as_bytes(), "{stamps:?} {layout:?}");
            assert_eq!(report.legacy_layout, Some(layout));
        }
        let mut reads = spy.lock().expect("source").reads();
        reads.sort();
        assert_eq!(reads, ["logo.png", "logo.png", "src/main.rs", "src/main.rs"]);
    }
    assert!(!SNAPSHOT.contains("MOD"));
    assert!(SNAPSHOT_STAMPED.contains("MOD:"));
}

#[test]
fn facade_diff_preserves_full_tree_ids_and_changed_content_only() {
    for stamps in [TreeStamps::Off, TreeStamps::On] {
        diff_case(stamps);
    }
}

fn diff_case(stamps: TreeStamps) {
    let mut source = MemorySource::new();
    source.insert_file("a.txt", b"unchanged", 1).expect("a");
    source.insert_file("changed.txt", b"old", 1).expect("changed");
    source.insert_file("icon.svg", b"<svg/>", 1).expect("svg");
    let source = SharedSource::new(source);
    let handle = source.handle();
    let mut session = session(source, None);
    let request = request_with(LegacyLayout::Diff, stamps);
    let mut first = VecSink::new();
    let report = session.pack(&request, &mut first).expect("first");
    assert_eq!(report.legacy_layout, Some(LegacyLayout::Snapshot));
    assert!(first.as_bytes().starts_with(b"[DCTX_LLM_CONTEXT_SNAPSHOT_V1]\n"));
    {
        let mut source = handle.lock().expect("source");
        source.insert_file("changed.txt", b"new\n", 2).expect("changed");
        // Same size, other bytes: a bare mtime change alone is no longer a modification (DCX-P-C1b).
        source.insert_file("icon.svg", b"<svG/>", 2).expect("svg");
    }
    let mut sink = VecSink::new();
    let report = session.pack(&request, &mut sink).expect("diff");
    assert_eq!(report.legacy_layout, Some(LegacyLayout::Diff));
    let (root, a, changed, icon) = match stamps {
        TreeStamps::Off => ("", "", "", ""),
        TreeStamps::On => (", MOD:0", ", MOD:1", ", MOD:2", ", MOD:2"),
    };
    let (size_a, size_changed, size_icon) = match stamps {
        TreeStamps::Off => ("", "", ""),
        TreeStamps::On => (", SIZE:9", ", SIZE:4", ", SIZE:6"),
    };
    let expected = format!(
        "[DCTX_LLM_CONTEXT_DIFF_V1.1]\nVersion Change: V1 -> V1.1\n\n\
<CHANGES_SUMMARY>\n\
[MODIFIED] changed.txt\n\
[MODIFIED] icon.svg\n\
</CHANGES_SUMMARY>\n\n\
<UPDATED_DIRECTORY_TREE>\n\
[D]  (ID:ROOT{root})\n  [F] a.txt (ID:F001{a}{size_a})\n  [F] changed.txt (ID:F002{changed}{size_changed})\n  [F] icon.svg (ID:F003{icon}{size_icon})</UPDATED_DIRECTORY_TREE>\n\n\
<FILE_CONTENT_START ID=\"F002\" PATH=\"changed.txt\" LINES=\"2\">\n\
new\n\n\
</FILE_CONTENT_END ID=\"F002\">\n\
<FILE_CONTENT_START ID=\"F003\" PATH=\"icon.svg\" LINES=\"0\">\n\
[SVG file, content omitted (omit_svg_content=true)]\n\
</FILE_CONTENT_END ID=\"F003\">\n"
    );
    assert_eq!(sink.as_bytes(), expected.as_bytes(), "{stamps:?}");
    let mut unchanged = VecSink::new();
    let report = session.pack(&request, &mut unchanged).expect("unchanged");
    assert!(!report.published);
    assert!(!std::str::from_utf8(unchanged.as_bytes()).expect("utf8").contains("FILE_CONTENT_START"));
    {
        let mut source = handle.lock().expect("source");
        for path in ["a.txt", "changed.txt", "icon.svg"] {
            source.insert_file(path, b"reset", 3).expect("reset");
        }
    }
    let mut reset = VecSink::new();
    let report = session.pack(&request, &mut reset).expect("major reset");
    assert!(report.major_reset);
    assert_eq!(report.legacy_layout, Some(LegacyLayout::Snapshot));
    assert!(reset.as_bytes().starts_with(b"[DCTX_LLM_CONTEXT_SNAPSHOT_V2]\n"));
}

#[test]
fn final_byte_cap_includes_header_and_utf8_and_never_writes_a_partial_page() {
    for (stamps, snapshot, _tree) in MODES {
        byte_cap_case(stamps, snapshot);
    }
}

fn byte_cap_case(stamps: TreeStamps, snapshot: &str) {
    let expected = snapshot.replace("Corpus", "Ž");
    let length = expected.len() as u64;
    let mut request = request_with(LegacyLayout::Snapshot, stamps);
    request.legacy_output.as_mut().expect("output").template.insert_variable("project", "Ž");
    for host_cap in [false, true] {
        let mut session = session(snapshot_source(), host_cap.then_some(length - 1));
        request.limits.encoded_bytes = Some(if host_cap { length + 100 } else { length - 1 });
        let mut refused = VecSink::new();
        assert_eq!(session.pack(&request, &mut refused), Err(DctxError::BudgetTooSmall { minimum_bytes: length }));
        assert!(refused.is_empty());
    }
    let mut session = session(snapshot_source(), Some(length));
    request.limits.encoded_bytes = Some(length);
    let mut accepted = VecSink::new();
    session.pack(&request, &mut accepted).expect("exact byte cap");
    assert_eq!(accepted.as_bytes(), expected.as_bytes());
}

#[test]
fn svg_settings_and_templates_are_injected_without_implicit_config_reads() {
    let mut source = MemorySource::new();
    source.insert_file("icon.svg", b"<svg/>", 0).expect("svg");
    source.insert_file(".dctx/config.json", b"invalid configuration", 0).expect("config");
    source.fail_reads_of(".dctx/config.json");
    source.fail_listing_of(".dctx");
    let source = SharedSource::new(source);
    let spy = source.handle();
    let mut session = session(source, None);
    let mut request = request(LegacyLayout::Snapshot);
    let output = request.legacy_output.as_mut().expect("output");
    output.debug_ignored = false;
    output.template = Template::parse("[PARTIAL:FILE_BLOCK]\n{{{file.lines}}}:{{{file.content}}}\n[LAYOUT]\n{{{comp.files}}}");
    let mut omitted = VecSink::new();
    session.pack(&request, &mut omitted).expect("omit");
    assert_eq!(omitted.as_bytes(), b"[DCTX_LLM_CONTEXT_SNAPSHOT_V1]\n0:[SVG file (omit_svg_content=true), content omitted]");
    request.legacy_output.as_mut().expect("output").omit_svg_content = false;
    let mut included = VecSink::new();
    session.pack(&request, &mut included).expect("include");
    assert_eq!(included.as_bytes(), b"[DCTX_LLM_CONTEXT_SNAPSHOT_V1]\n1:<svg/>");
    let spy = spy.lock().expect("source");
    assert_eq!(spy.reads(), ["icon.svg", "icon.svg"]);
    assert!(!spy.touched(".dctx"));
}

#[test]
fn summaries_remain_the_default() {
    let mut session = session(MemorySource::new(), None);
    let mut request = PackRequest::new();
    assert!(request.legacy_output.is_none());
    request.output_format = 1;
    let mut text = VecSink::new();
    let report = session.pack(&request, &mut text).expect("text");
    assert_eq!(report.legacy_layout, None);
    assert_eq!(text.as_bytes(), b"# pack V1 files=0 changes=0\n");
    request.output_format = 2;
    let mut json = VecSink::new();
    session.pack(&request, &mut json).expect("json");
    assert!(json.as_bytes().starts_with(b"{\"v\":2,\"version\":\"V1\",\"previous\":\"V1\""));
    assert!(!std::str::from_utf8(json.as_bytes()).expect("utf8").contains("legacy"));
}

fn repeated_content_request(times: usize, cap: u64) -> PackRequest {
    let partial = "{{{file.content}}}".repeat(times);
    let mut raw = String::from("[PARTIAL:FILE_BLOCK]\n");
    raw.push_str(&partial);
    raw.push_str("\n[LAYOUT]\n{{{comp.files}}}");
    let mut request = PackRequest::new();
    request.legacy_output = Some(LegacyOutput::new(Template::parse(&raw), LegacyLayout::Snapshot));
    request.limits.encoded_bytes = Some(cap);
    request
}

fn capped_text_session(bytes: &[u8], cap: u64) -> Session {
    let mut source = MemorySource::new();
    source.insert_file("note.txt", bytes, 1).expect("file");
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: 64 * 1024 * 1024 })
        .limits(OperationLimits { encoded_bytes: Some(cap), ..OperationLimits::default() })
        .place("fixture")
        .view("pack")
        .build()
        .expect("session")
}

#[test]
fn legacy_byte_cap_bounds_file_block_expansion() {
    // Positive control: one copy of 40 bytes fits an exact 71-byte cap and is written whole.
    let header = "[DCTX_LLM_CONTEXT_SNAPSHOT_V1]\n";
    let mut expected = header.as_bytes().to_vec();
    expected.extend(std::iter::repeat_n(b'B', 40));
    assert_eq!(expected.len(), 71);
    let mut session = capped_text_session(&expected[header.len()..], 71);
    let mut sink = VecSink::new();
    session.pack(&repeated_content_request(1, 71), &mut sink).expect("exact fit");
    assert_eq!(sink.as_bytes(), expected.as_slice());

    let mut session = capped_text_session(&expected[header.len()..], 70);
    let mut refused = VecSink::new();
    assert_eq!(
        session.pack(&repeated_content_request(1, 70), &mut refused),
        Err(DctxError::BudgetTooSmall { minimum_bytes: 71 })
    );
    assert!(refused.is_empty());

    // F5: 4096 copies of an 8 MiB file expand to 32 GiB before a 1 KiB cap can be applied.
    let content = vec![b'A'; 8 * 1024 * 1024];
    let minimum = header.len() as u64 + 4096 * content.len() as u64;
    let mut session = capped_text_session(&content, 1024);
    let mut bomb = VecSink::new();
    assert_eq!(
        session.pack(&repeated_content_request(4096, 1024), &mut bomb),
        Err(DctxError::BudgetTooSmall { minimum_bytes: minimum })
    );
    assert!(bomb.is_empty());
}
