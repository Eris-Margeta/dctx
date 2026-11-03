// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn child_hash_matches_the_dotted_name() {
    let root = fnv(FNV_OFFSET, b"workspace");
    let child = fnv(fnv(root, b"."), b"dependencies");
    assert_eq!(child, fnv(FNV_OFFSET, b"workspace.dependencies"));
    assert_eq!(fnv(FNV_OFFSET, b"0"), fnv(FNV_OFFSET, b"0"));
}

fn symbol(start: u32, end: u32, hash_name: &str) -> DraftNode {
    DraftNode {
        name_start: start,
        name_end: end,
        name_hash: fnv(FNV_OFFSET, hash_name.as_bytes()),
        parent: 0,
        start: 0,
        end: 1,
        precision_end: 1,
        lines: [1, 1],
        container: false,
        opened: true,
        array: false,
        noted: false,
        module: false,
        symbol_kind: None,
        signature: Stored::Empty,
        summary_text: Stored::Empty,
        summary_source: SummarySource::Signature,
    }
}

#[test]
fn name_index_keeps_distinct_paths_that_share_a_hash() {
    let buf = "a.ba.c";
    let nodes = vec![symbol(0, 3, "a.b"), symbol(3, 6, "a.c")];
    let mut names = NameIndex::with_capacity(2);
    names.insert(7, 0);
    names.insert(7, 1);
    assert_eq!(names.find(7, "a.b", &nodes, buf), Some(0));
    assert_eq!(names.find(7, "a.c", &nodes, buf), Some(1));
    assert_eq!(names.find(7, "missing", &nodes, buf), None);
    assert_eq!(names.find(8, "a.b", &nodes, buf), None);
}

#[test]
fn rejected_names_skip_before_a_fact_is_charged() {
    let mut draft = Draft::open("a.json", r#"{"a#b":1,"a~1":2}"#, 8, 8).unwrap();
    assert!(draft.put(0, "a#b", Kind::Value(false), 1, 2, 1).unwrap().index().is_none());
    assert!(draft.put(0, "a~1", Kind::Value(false), 1, 2, 1).unwrap().index().is_none());
    assert!(draft.put(0, "", Kind::Value(false), 1, 2, 1).unwrap().index().is_none());
    let file = draft.finish(Dialect::Json).unwrap();
    assert_eq!(file.nodes.len(), 1);
    assert_eq!(file.completion, Completion::Partial);
}

#[test]
fn spanned_signature_matches_the_bounded_string() {
    let source = r#"{"n":"hello"}"#;
    let mut draft = Draft::open("a.json", source, 8, 8).unwrap();
    let index = draft.put(0, "n", Kind::Value(false), 5, 12, 1).unwrap().index().unwrap();
    draft.set_signature(index, &source[6..11]);
    let file = draft.finish(Dialect::Json).unwrap();
    assert_eq!(file.nodes[1].fact.signature, "hello");
    assert_eq!(file.nodes[1].fact.summary.text, "hello");
    assert_eq!(
        file.nodes[1].fact.id,
        StableId::new(NodeKind::Symbol, "a.json", Some("n"), None).unwrap()
    );
    let mut spaced = Draft::open("a.json", source, 8, 8).unwrap();
    let index = spaced.put(0, "n", Kind::Value(false), 5, 12, 1).unwrap().index().unwrap();
    spaced.set_signature(index, "a  b\tc");
    let file = spaced.finish(Dialect::Json).unwrap();
    assert_eq!(file.nodes[1].fact.signature, "a b c");
}

fn wide(source: &str) -> Draft<'_> {
    let mut draft = Draft::open("dir/a.json", source, 100_000, 8).unwrap();
    for index in 0..3000 {
        let name = format!("k{index}");
        let Some(at) = draft.put(0, &name, Kind::Value(false), 0, source.len(), 1).unwrap().index() else {
            continue;
        };
        draft.set_signature(at, &source[..source.len().min(4)]);
    }
    draft
}

#[test]
fn parallel_finish_matches_one_worker() {
    for source in ["abcdefghij", "a b\tc d"] {
        let one = wide(source).finish_with(Dialect::Json, 1).unwrap();
        let many = wide(source).finish_with(Dialect::Json, 4).unwrap();
        assert_eq!(one, many, "{source:?}");
        assert_eq!(one.nodes.len(), 3001);
        assert_eq!(one.edges.len(), 3000);
    }
}
