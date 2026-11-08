// SPDX-License-Identifier: Apache-2.0
//! Tests for [`super`]: edge direction, the sibling rule, tie-breaks, the hop cap and the pages.

use super::*;
use crate::domain::FreshnessMode;
use crate::index::{IndexedEdge, IndexedNode, TextBlob};

/// A generation from `(letter, kind, name, path)` nodes and `(src, dst, edge)` edges.
fn build(nodes: &[(u8, &str, &str, &str)], edges: &[(u32, u32, &str)]) -> Generation {
    let mut text = TextBlob::default();
    let nodes = nodes
        .iter()
        .map(|&(letter, kind, name, path)| IndexedNode {
            letter,
            ordinal: 0,
            kind: kind.to_string().into(),
            name: name.to_string().into(),
            path: text.push(path),
            lines: [1, 1],
            span: [0, 0],
            summary: String::new().into(),
            summary_source: String::new().into(),
            signature: String::new().into(),
        })
        .collect();
    let exact = text.push("exact");
    let edges = edges
        .iter()
        .map(|&(src, dst, name)| IndexedEdge {
            src,
            dst,
            edge: text.push(name),
            confidence: exact,
        })
        .collect();
    Generation {
        id: 1,
        digest: "v1:sha256:ab:1".to_string(),
        encoded_len: 0,
        files: Vec::new(),
        nodes,
        edges,
        text,
        freshness: FreshnessMode::Strict,
        taken_at: 0,
        epoch: 0,
        coverage: "structural".to_string(),
        completion: "complete".to_string(),
        parser: "p".to_string(),
        ids: crate::index::IdIndex::default(),
    }
}

/// One file with `a1 -> a2 -> a3` and an unconnected `lone`, all contained by the module.
fn chain() -> Generation {
    let nodes = [
        (b'm', "module", "", "a.py"),
        (b's', "fn", "a1", "a.py"),
        (b's', "fn", "a2", "a.py"),
        (b's', "fn", "a3", "a.py"),
        (b's', "fn", "lone", "a.py"),
    ];
    build(
        &nodes,
        &[
            (0, 1, "contains"),
            (0, 2, "contains"),
            (0, 3, "contains"),
            (0, 4, "contains"),
            (1, 2, "calls"),
            (2, 3, "calls"),
        ],
    )
}

/// Hops as `edge:direction id` strings.
fn shown(hops: &Option<Vec<Hop>>) -> Vec<String> {
    hops.as_ref().map_or_else(Vec::new, |hops| {
        hops.iter()
            .map(|hop| format!("{}:{} {}", hop.edge, hop.direction, hop.id))
            .collect()
    })
}

#[test]
fn the_path_follows_calls_and_never_hops_between_siblings_through_the_module() {
    let generation = chain();
    let forward = shortest_path(&generation, "s:a.py#a1", "s:a.py#a3").expect("known");
    assert_eq!(
        shown(&forward),
        [": s:a.py#a1", "calls:out s:a.py#a2", "calls:out s:a.py#a3"]
    );
    let back = shortest_path(&generation, "s:a.py#a3", "s:a.py#a1").expect("known");
    assert_eq!(
        shown(&back),
        [": s:a.py#a3", "calls:in s:a.py#a2", "calls:in s:a.py#a1"]
    );
    assert_eq!(
        shortest_path(&generation, "s:a.py#a1", "s:a.py#lone").expect("known"),
        None
    );
    let down = shortest_path(&generation, "m:a.py", "s:a.py#a3").expect("known");
    assert_eq!(shown(&down), [": m:a.py", "contains:out s:a.py#a3"]);
    let up = shortest_path(&generation, "s:a.py#lone", "m:a.py").expect("known");
    assert_eq!(shown(&up), [": s:a.py#lone", "contains:in m:a.py"]);
    let same = shortest_path(&generation, "s:a.py#a1", "s:a.py#a1").expect("known");
    assert_eq!(shown(&same), [": s:a.py#a1"]);
    assert_eq!(
        shortest_path(&generation, "s:a.py#gone", "s:a.py#a1"),
        Err("s:a.py#gone".to_string())
    );
    assert_eq!(
        shortest_path(&generation, "s:a.py#a1", "s:nope#x"),
        Err("s:nope#x".to_string())
    );
}

#[test]
fn equal_paths_choose_the_smaller_node_ids() {
    let nodes = [
        (b's', "fn", "x", "t.py"),
        (b's', "fn", "c", "t.py"),
        (b's', "fn", "b", "t.py"),
        (b's', "fn", "z", "t.py"),
    ];
    let generation = build(
        &nodes,
        &[
            (0, 1, "calls"),
            (0, 2, "calls"),
            (1, 3, "calls"),
            (2, 3, "calls"),
        ],
    );
    let hops = shortest_path(&generation, "s:t.py#x", "s:t.py#z").expect("known");
    assert_eq!(
        shown(&hops),
        [": s:t.py#x", "calls:out s:t.py#b", "calls:out s:t.py#z"]
    );
}

#[test]
fn a_path_may_have_eight_hops_and_no_more() {
    let names: Vec<String> = (0..10).map(|n| format!("f{n}")).collect();
    let nodes: Vec<(u8, &str, &str, &str)> = names
        .iter()
        .map(|name| (b's', "fn", name.as_str(), "c.py"))
        .collect();
    let edges: Vec<(u32, u32, &str)> = (0..9).map(|n| (n, n + 1, "imports")).collect();
    let generation = build(&nodes, &edges);
    let eight = shortest_path(&generation, "s:c.py#f0", "s:c.py#f8")
        .expect("known")
        .expect("8 hops");
    assert_eq!(eight.len(), 9);
    assert_eq!(
        shortest_path(&generation, "s:c.py#f0", "s:c.py#f9").expect("known"),
        None
    );
    assert_eq!(
        shortest_path(&generation, "s:c.py#f9", "s:c.py#f1")
            .expect("known")
            .map(|hops| hops.len()),
        Some(9)
    );
}

#[test]
fn pages_list_the_hops_or_say_no_path() {
    let generation = chain();
    let head = PageHead {
        place: "fixture",
        snapshot: &generation.digest,
        generation: 1,
        mode: "immutable_snapshot",
        state: "verified",
        age_s: 0,
        coverage: "structural",
        completion: "complete",
        parser: "p",
        meta: false,
    };
    let hops = shortest_path(&generation, "s:a.py#a1", "s:a.py#a3").expect("known");
    let text = encode(&head, hops.as_deref(), false);
    assert_eq!(
        text,
        "# fixture path\nstart\ts:a.py#a1\tfn\ta.py\ncalls:out\ts:a.py#a2\tfn\ta.py\ncalls:out\ts:a.py#a3\tfn\ta.py\n"
    );
    assert_eq!(encode(&head, None, false), "# fixture path\nno path\n");
    let json = encode(&head, hops.as_deref(), true);
    assert!(
        json.contains("\"verb\":\"path\"") && json.contains("\"found\":true"),
        "{json}"
    );
    assert!(json.contains("{\"id\":\"s:a.py#a2\",\"kind\":\"fn\",\"path\":\"a.py\",\"edge\":\"calls\",\"direction\":\"out\"}"), "{json}");
    assert!(encode(&head, None, true).contains("\"found\":false,\"records\":[]"));
}
