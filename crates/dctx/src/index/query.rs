//! Ranking and selection for the five verbs. Page fitting stays with the session.
//! Implements contracts/API-PORTS-v1.md record order: score descending, id ascending.
//! Depends on the in-memory generation. Map, find and neighbors do not read the source.
//! A missing id is [`Collected::Outcome`], not an empty success.

use super::{Generation, IndexedNode};
use crate::domain::{AnswerRecord, Direction, IdentityOutcome, MapRequest, Verb, DctxError};

/// Records for a page, a verified read, or a normal identity outcome.
#[derive(Debug)]
pub(crate) enum Collected {
    /// Ranked records, best first.
    Records(Vec<AnswerRecord>),
    /// A node whose bytes must be re-read and checked before emission.
    Read(Box<AnswerRecord>),
    /// Gone or ambiguous. The session sets the next action.
    Outcome(IdentityOutcome),
}

/// Selects records for `request` from `generation`.
pub(crate) fn collect(generation: &Generation, request: &MapRequest, depth: u32) -> Result<Collected, DctxError> {
    match request.verb {
        Verb::Find => Ok(Collected::Records(find(generation, request))),
        Verb::Map => map_children(generation, request, depth),
        Verb::Neighbors => neighbors(generation, request, depth),
        Verb::Explain => explain(generation, request),
        Verb::Read => read(generation, request),
    }
}

/// The node named `target`, or `None` when it is absent. An `@` alias is never guessed.
pub(crate) fn lookup<'a>(nodes: &'a [IndexedNode], target: &str) -> Option<&'a IndexedNode> {
    nodes.iter().find(|node| node.id == target)
}

fn map_children(generation: &Generation, request: &MapRequest, depth: u32) -> Result<Collected, DctxError> {
    let Some(root) = require(&generation.nodes, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut frontier = vec![root.id.clone()];
    let mut children = Vec::new();
    for _ in 0..depth {
        let mut next = Vec::new();
        for id in &frontier {
            for edge in &generation.edges {
                if edge.src == *id && edge.edge == "contains" {
                    next.push(edge.dst.clone());
                }
            }
        }
        children.extend(next.iter().cloned());
        frontier = next;
    }
    let mut records = Vec::new();
    for id in children {
        if let Some(node) = lookup(&generation.nodes, &id)
            && admit(node, request)
        {
            records.push(record_of(node));
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(Collected::Records(records))
}

fn find(generation: &Generation, request: &MapRequest) -> Vec<AnswerRecord> {
    let mut records = Vec::new();
    for node in &generation.nodes {
        if node.kind == "module" || !admit(node, request) {
            continue;
        }
        let score = score_of(node, &request.query);
        if score == 0 {
            continue;
        }
        let mut record = record_of(node);
        record.score = score;
        records.push(record);
    }
    records.sort_by(|left, right| right.score.cmp(&left.score).then(left.id.cmp(&right.id)));
    records
}

fn neighbors(generation: &Generation, request: &MapRequest, depth: u32) -> Result<Collected, DctxError> {
    let Some(root) = require(&generation.nodes, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut frontier = vec![root.id.clone()];
    let mut records = Vec::new();
    for _ in 0..depth {
        let mut next = Vec::new();
        for id in &frontier {
            for edge in &generation.edges {
                let (direction, other) = if edge.src == *id {
                    ("out", edge.dst.as_str())
                } else if edge.dst == *id {
                    ("in", edge.src.as_str())
                } else {
                    continue;
                };
                if !direction_ok(request.direction, direction) {
                    continue;
                }
                if !request.edges.is_empty() && !request.edges.iter().any(|kind| kind == &edge.edge) {
                    continue;
                }
                let Some(node) = lookup(&generation.nodes, other) else {
                    continue;
                };
                if !admit(node, request) {
                    continue;
                }
                let mut record = record_of(node);
                record.id = format!("{}:{direction}:{}", edge.edge, node.id);
                record.edge = Some(edge.edge.clone());
                record.direction = Some(direction.to_string());
                record.target = Some(node.id.clone());
                record.confidence = Some(edge.confidence.clone());
                records.push(record);
                next.push(node.id.clone());
            }
        }
        frontier = next;
    }
    records.sort_by(|left, right| {
        left.edge.cmp(&right.edge).then(left.target.cmp(&right.target)).then(left.id.cmp(&right.id))
    });
    records.dedup_by(|left, right| left.id == right.id);
    Ok(Collected::Records(records))
}

fn explain(generation: &Generation, request: &MapRequest) -> Result<Collected, DctxError> {
    let Some(node) = require(&generation.nodes, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut record = record_of(node);
    if !node.signature.is_empty() {
        record.signature = Some(node.signature.clone());
    }
    Ok(Collected::Records(vec![record]))
}

fn read(generation: &Generation, request: &MapRequest) -> Result<Collected, DctxError> {
    let Some(node) = require(&generation.nodes, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut record = record_of(node);
    record.span = Some(node.span);
    Ok(Collected::Read(Box::new(record)))
}

fn require<'a>(nodes: &'a [IndexedNode], target: &str) -> Result<Option<&'a IndexedNode>, DctxError> {
    if target.is_empty() {
        return Err(DctxError::invalid("target is required"));
    }
    Ok(lookup(nodes, target))
}

fn missing(target: &str) -> Collected {
    Collected::Outcome(IdentityOutcome::Gone { id: target.to_string() })
}

fn admit(node: &IndexedNode, request: &MapRequest) -> bool {
    if !request.kinds.is_empty() && !request.kinds.iter().any(|kind| kind == &node.kind) {
        return false;
    }
    match &request.within {
        Some(prefix) => node.path == *prefix || node.path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/')),
        None => true,
    }
}

fn direction_ok(requested: Direction, actual: &str) -> bool {
    match requested {
        Direction::Both => true,
        Direction::Out => actual == "out",
        Direction::In => actual == "in",
    }
}

fn record_of(node: &IndexedNode) -> AnswerRecord {
    AnswerRecord {
        id: node.id.clone(),
        kind: node.kind.clone(),
        path: node.path.clone(),
        lines: node.lines,
        summary: node.summary.clone(),
        summary_source: node.summary_source.clone(),
        signature: None,
        span: None,
        source_utf8: None,
        edge: None,
        direction: None,
        target: None,
        confidence: None,
        score: 0,
    }
}

fn score_of(node: &IndexedNode, query: &str) -> i64 {
    if query.is_empty() {
        return 0;
    }
    if node.name == query {
        return 3000;
    }
    if node.name.strip_suffix(query).is_some_and(|head| head.ends_with("::")) {
        return 2500;
    }
    if node.name.contains(query) {
        return 1000;
    }
    if node.path.contains(query) {
        return 100;
    }
    if node.summary.contains(query) {
        return 10;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_score_prefers_an_exact_name() {
        let node = IndexedNode {
            id: "s:src/lib.rs#ping".to_string(),
            kind: "fn".to_string(),
            name: "ping".to_string(),
            path: "src/lib.rs".to_string(),
            lines: [2, 2],
            span: [13, 29],
            summary: "Respond.".to_string(),
            summary_source: "doc".to_string(),
            signature: "pub fn ping()".to_string(),
        };
        assert_eq!(score_of(&node, "ping"), 3000);
        assert_eq!(score_of(&node, "src/lib"), 100);
        assert_eq!(score_of(&node, "Respond"), 10);
        assert_eq!(score_of(&node, "missing"), 0);
        let qualified = IndexedNode { name: "tests::ping".to_string(), ..node };
        assert_eq!(score_of(&qualified, "ping"), 2500);
    }
}
