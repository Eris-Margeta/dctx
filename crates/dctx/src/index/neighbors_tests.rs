//! Tests for the breadth-first neighbors traversal over a hand-built call chain `f1 -> f2 -> f3 -> f4` plus a back edge `f4 -> f1`.

use super::{collect, Collected};
use crate::domain::{Direction, FreshnessMode, MapRequest, Verb};
use crate::index::{Generation, IndexedEdge, IndexedNode, TextBlob};

/// A generation with four functions in `a.py`, a call chain and one back edge.
fn chain() -> Generation {
    let mut text = TextBlob::default();
    let path = text.push("a.py");
    let nodes = (1..=4)
        .map(|n| IndexedNode {
            letter: b's',
            ordinal: 0,
            kind: "fn".to_string(),
            name: format!("f{n}"),
            path,
            lines: [n, n],
            span: [0, 0],
            summary: String::new(),
            summary_source: "sig".to_string(),
            signature: String::new(),
        })
        .collect();
    let calls = text.push("calls");
    let name = text.push("name");
    let edges = Vec::from([(0, 1), (1, 2), (2, 3), (3, 0)].map(|(src, dst)| IndexedEdge { src, dst, edge: calls, confidence: name }));
    Generation {
        id: 1,
        digest: String::new(),
        encoded_len: 0,
        files: Vec::new(),
        nodes,
        edges,
        text,
        freshness: FreshnessMode::ImmutableSnapshot,
        taken_at: 0,
        epoch: 0,
        coverage: "structural".to_string(),
        completion: "complete".to_string(),
        parser: String::new(),
    }
}

/// Record ids of an outbound walk from `f1` to `depth`.
fn ids(depth: u32) -> Vec<String> {
    let mut request = MapRequest::new(Verb::Neighbors);
    request.target = "s:a.py#f1".to_string();
    request.direction = Direction::Out;
    let Collected::Records(records) = collect(&chain(), &request, depth).expect("collect") else {
        return Vec::new();
    };
    records.into_iter().map(|record| record.id).collect()
}

#[test]
fn depth_one_lines_keep_their_shape() {
    assert_eq!(ids(1), ["calls:out:s:a.py#f2"]);
}

#[test]
fn deeper_nodes_follow_by_depth_once_and_carry_the_depth() {
    assert_eq!(ids(3), ["calls:out:s:a.py#f2", "calls:out:2:s:a.py#f3", "calls:out:3:s:a.py#f4"]);
    // The back edge reaches f1, which is the root and already settled.
    assert_eq!(ids(4), ids(3));
}

/// A self call (`f1 -> f1`) is listed for an inbound walk as well as an outbound one.
#[test]
fn a_self_edge_is_listed_inbound() {
    let mut generation = chain();
    let calls = generation.edges[0].edge;
    let name = generation.edges[0].confidence;
    generation.edges = vec![IndexedEdge { src: 0, dst: 0, edge: calls, confidence: name }];
    for (direction, want) in [(Direction::In, "calls:in:s:a.py#f1"), (Direction::Out, "calls:out:s:a.py#f1")] {
        let mut request = MapRequest::new(Verb::Neighbors);
        request.target = "s:a.py#f1".to_string();
        request.direction = direction;
        let Collected::Records(records) = collect(&generation, &request, 1).expect("collect") else { panic!("records") };
        assert_eq!(records.into_iter().map(|record| record.id).collect::<Vec<_>>(), [want]);
    }
}
