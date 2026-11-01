//! Pack prefix stability (DCX-P-C3): the default legacy page keeps its bytes across a bare touch
//! and across an edit to one late file, so a prompt cache can reuse the shared prefix.
//! Twenty equal-size files, fixed clock, the public facade with the built-in default template.
//! `--tree-stamps=on` is the contrast: its tree lines carry `MOD:`, so a touch breaks the prefix early.
//! Run by `cargo test -p dctx --test pack_prefix`.

#![forbid(unsafe_code)]

use std::sync::Arc;

use dctx::memory::{CancelFlag, FixedClock, MemorySource};
use dctx::{
    HostCeiling, HostGrants, PackRequest, RuleTexts, SelectionPolicy, Session, StoragePolicy,
    Timestamp, VecSink,
};
use dctx_template::{LegacyLayout, LegacyOutput, TreeStamps};

const FILES: usize = 20;

fn path(number: usize) -> String {
    format!("src/f{number:02}.txt")
}

/// A 522-byte body that depends on `number` and `revision`, so edits keep the size.
fn body(number: usize, revision: u8) -> Vec<u8> {
    let line =
        format!("file {number:02} revision {revision} lorem ipsum dolor sit amet consectetur\n");
    line.repeat(9).into_bytes()
}

struct Fixture {
    revisions: [u8; FILES],
    mtimes: [u64; FILES],
}

impl Fixture {
    fn new() -> Self {
        Self {
            revisions: [0; FILES],
            mtimes: [1_000; FILES],
        }
    }

    fn page(&self, stamps: TreeStamps) -> String {
        let mut source = MemorySource::new();
        for number in 1..=FILES {
            let index = number - 1;
            source
                .insert_file(
                    &path(number),
                    &body(number, self.revisions[index]),
                    self.mtimes[index],
                )
                .expect("file");
        }
        let mut session = Session::builder(source)
            .clock(FixedClock(Timestamp(0)))
            .cancellation(Arc::new(CancelFlag::new()))
            .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
            .ceiling(HostCeiling::base())
            .grants(HostGrants::PermitAll)
            .storage(StoragePolicy::Ephemeral {
                max_bytes: 8 * 1024 * 1024,
            })
            .place("fixture")
            .view("pack")
            .build()
            .expect("session");
        let mut template = stamps.template();
        template.insert_variable("project", "prefix");
        let request = PackRequest {
            legacy_output: Some(LegacyOutput::new(template, LegacyLayout::Snapshot)),
            ..PackRequest::new()
        };
        let mut sink = VecSink::new();
        session.pack(&request, &mut sink).expect("pack");
        String::from_utf8(sink.as_bytes().to_vec()).expect("utf8")
    }
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

#[test]
fn off_page_survives_a_touch_and_keeps_the_prefix_across_an_edit() {
    let mut fixture = Fixture::new();
    let base = fixture.page(TreeStamps::Off);
    assert_eq!(
        base.matches("MOD").count(),
        0,
        "the default page carries no timestamp"
    );
    assert!(base.contains("PATH=\"src/f15.txt\"") && base.contains("PATH=\"src/f20.txt\""));

    // Touch file 3: the mtime moves, the bytes do not.
    fixture.mtimes[2] += 86_400_000;
    assert_eq!(
        fixture.page(TreeStamps::Off),
        base,
        "a bare touch must leave the page identical"
    );

    // Edit file 15 (same size, new bytes): everything before its block is shared.
    fixture.revisions[14] = 1;
    fixture.mtimes[14] += 5_000;
    let edited = fixture.page(TreeStamps::Off);
    assert_ne!(edited, base);
    let shared = common_prefix(&base, &edited);
    let percent = shared * 100 / base.len();
    assert!(
        percent >= 70,
        "common prefix {shared} of {} bytes is {percent} %, wanted >= 70 %",
        base.len()
    );
    // The first difference is inside file 15's content block, never in the tree.
    let block = base.find("PATH=\"src/f15.txt\"").expect("block 15");
    assert!(
        shared > block,
        "shared prefix ends at {shared}, before block 15 at {block}"
    );
    assert_eq!(
        edited.len(),
        base.len(),
        "an equal-size edit keeps the length"
    );
}

#[test]
fn on_page_shows_why_the_default_dropped_the_stamps() {
    let mut fixture = Fixture::new();
    let base = fixture.page(TreeStamps::On);
    assert!(
        base.contains("src/f03.txt (ID:F004, MOD:1000, SIZE:"),
        "the stamped page carries MOD"
    );
    fixture.mtimes[2] += 86_400_000;
    let touched = fixture.page(TreeStamps::On);
    assert_ne!(touched, base, "with stamps on, a touch changes the page");
    assert!(
        common_prefix(&base, &touched) * 100 / base.len() < 20,
        "and it breaks the prefix inside the tree"
    );
}
