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

/// A generation with `s:big.ts#target` and `count` callers named `caller<n>`, each with a 2 KB signature summary.
fn big_callers(count: u32) -> Generation {
    let mut generation = chain();
    generation.nodes.clear();
    generation.edges.clear();
    let path = generation.text.push("big.ts");
    let calls = generation.text.push("calls");
    let name = generation.text.push("name");
    for n in 0..=count {
        let label = if n == 0 { "target".to_string() } else { format!("caller{n}") };
        let sig = format!("export function {label}(): number {{ return {} }}", "x".repeat(2000));
        generation.nodes.push(IndexedNode {
            letter: b's',
            ordinal: 0,
            kind: "fn".to_string(),
            name: label,
            path,
            lines: [n + 1, n + 1],
            span: [0, 0],
            summary: sig,
            summary_source: "sig".to_string(),
            signature: String::new(),
        });
        if n > 0 {
            generation.edges.push(IndexedEdge { src: n, dst: 0, edge: calls, confidence: name });
        }
    }
    generation
}

#[test]
fn a_2000_token_page_carries_at_least_36_callers_with_2kb_summaries() {
    use crate::domain::{encode_answer, Answer, Freshness, Precision};
    let mut request = MapRequest::new(Verb::Neighbors);
    request.target = "s:big.ts#target".to_string();
    request.direction = Direction::In;
    let Collected::Records(records) = collect(&big_callers(60), &request, 1).expect("collect") else {
        panic!("records expected");
    };
    assert_eq!(records.len(), 60);
    assert!(records.iter().all(|record| record.summary.is_empty()), "a signature summary is dropped from an edge record");
    let page = |count: usize| -> String {
        let answer = Answer {
            generation: 1,
            snapshot: String::new(),
            verb: Verb::Neighbors,
            records: records[..count].to_vec(),
            more: 0,
            next: None,
            freshness: Freshness { mode: "immutable_snapshot".to_string(), state: "verified".to_string(), age_s: 0 },
            precision: Precision { coverage: "structural".to_string(), completion: "complete".to_string(), parser: "dctx-structural/2".to_string() },
            outcome: None,
        };
        encode_answer("fixture", &answer)
    };
    // 2000 tokens at 3.5 bytes each.
    let fitting = (1..=records.len()).take_while(|count| page(*count).len() <= 7000).count();
    assert!(fitting >= 36, "only {fitting} callers fit in 2000 tokens");
    let json = page(1);
    assert!(!json.contains("\"span\"") && !json.contains("\"signature\"") && !json.contains("\"target\""), "{json}");
    assert!(json.contains("\"edge\":\"calls\",\"direction\":\"in\",\"confidence\":\"name\""), "{json}");
}
