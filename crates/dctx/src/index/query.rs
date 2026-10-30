//! Ranking and selection for the five verbs. Page fitting stays with the session.
//! Implements contracts/API-PORTS-v1.md record order: score descending, id ascending.
//! Depends on the in-memory generation. Map, find and neighbors do not read the source.
//! A missing id is [`Collected::Outcome`], not an empty success.

use super::extract::language_of;
use super::{generic, id_matches, write_node_id, Generation, IndexedNode, TextBlob};
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
        // Grep reads file bytes, so the session answers it through `grep_records`.
        Verb::Grep => Ok(Collected::Records(Vec::new())),
        Verb::Skeleton => super::skeleton::skeleton(generation, request),
    }
}

/// The node named `target`, or `None` when it is absent. An `@` alias is never guessed.
pub(crate) fn lookup<'a>(nodes: &'a [IndexedNode], text: &TextBlob, target: &str) -> Option<&'a IndexedNode> {
    let index = lookup_index(nodes, text, target)?;
    nodes.get(index)
}

fn lookup_index(nodes: &[IndexedNode], text: &TextBlob, target: &str) -> Option<usize> {
    nodes.iter().position(|node| id_matches(text, node, target))
}

fn node_at(nodes: &[IndexedNode], index: u32) -> Option<&IndexedNode> {
    nodes.get(usize::try_from(index).unwrap_or(usize::MAX))
}

fn map_children(generation: &Generation, request: &MapRequest, depth: u32) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(root) = require_index(&generation.nodes, text, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut frontier = vec![root];
    let mut children = Vec::new();
    for _ in 0..depth {
        let mut next = Vec::new();
        for id in &frontier {
            for edge in &generation.edges {
                if edge.src == *id && text.get(edge.edge) == "contains" {
                    next.push(edge.dst);
                }
            }
        }
        children.extend_from_slice(&next);
        frontier = next;
    }
    let mut records = Vec::new();
    for id in children {
        if let Some(node) = node_at(&generation.nodes, id)
            && admit(text, node, request)
        {
            records.push(record_of(text, node));
        }
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(Collected::Records(records))
}

fn find(generation: &Generation, request: &MapRequest) -> Vec<AnswerRecord> {
    let text = &generation.text;
    let mut records = Vec::new();
    for node in &generation.nodes {
        if node.kind == "module" || !admit(text, node, request) {
            continue;
        }
        let score = score_of(text, node, &request.query);
        if score == 0 {
            continue;
        }
        let mut record = record_of(text, node);
        record.score = score;
        records.push(record);
    }
    records.sort_by(|left, right| right.score.cmp(&left.score).then(left.id.cmp(&right.id)));
    records
}

/// Breadth-first neighbors. Each node is listed once, at its smallest depth, levels in order, each level by edge, target and id.
/// A line deeper than one carries its depth after the direction, as `calls:in:2:<id>`; depth-1 lines keep `calls:in:<id>`.
fn neighbors(generation: &Generation, request: &MapRequest, depth: u32) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(root) = require_index(&generation.nodes, text, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut frontier = vec![root];
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut records = Vec::new();
    for level in 1..=depth {
        let mut found = Vec::new();
        let mut next: Vec<u32> = Vec::new();
        let mut queued: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for id in &frontier {
            for edge in &generation.edges {
                let (direction, other) = if edge.src == *id {
                    ("out", edge.dst)
                } else if edge.dst == *id {
                    ("in", edge.src)
                } else {
                    continue;
                };
                if !direction_ok(request.direction, direction) || seen.contains(&other) {
                    continue;
                }
                let edge_name = text.get(edge.edge);
                if !request.edges.is_empty() && !request.edges.iter().any(|kind| kind.as_str() == edge_name) {
                    continue;
                }
                let Some(node) = node_at(&generation.nodes, other) else {
                    continue;
                };
                if !admit(text, node, request) {
                    continue;
                }
                let mut record = record_of(text, node);
                let node_id = record.id.clone();
                record.id = if level > 1 { format!("{edge_name}:{direction}:{level}:{node_id}") } else { format!("{edge_name}:{direction}:{node_id}") };
                record.edge = Some(edge_name.to_string());
                record.direction = Some(direction.to_string());
                record.target = Some(node_id);
                record.confidence = Some(text.get(edge.confidence).to_string());
                found.push(record);
                if queued.insert(other) {
                    next.push(other);
                }
            }
        }
        found.sort_by(|left, right| left.edge.cmp(&right.edge).then(left.target.cmp(&right.target)).then(left.id.cmp(&right.id)));
        found.dedup_by(|left, right| left.id == right.id);
        records.append(&mut found);
        // The root stays listable at depth one (a self call); from depth two on it and every earlier node are settled.
        seen.insert(root);
        seen.extend(next.iter().copied());
        frontier = next;
    }
    Ok(Collected::Records(records))
}

fn explain(generation: &Generation, request: &MapRequest) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(node) = require(&generation.nodes, text, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut record = record_of(text, node);
    if !node.signature.is_empty() {
        record.signature = Some(node.signature.clone());
    }
    Ok(Collected::Records(vec![record]))
}

fn read(generation: &Generation, request: &MapRequest) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(node) = require(&generation.nodes, text, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut record = record_of(text, node);
    record.span = Some(node.span);
    Ok(Collected::Read(Box::new(record)))
}

pub(super) fn require<'a>(nodes: &'a [IndexedNode], text: &TextBlob, target: &str) -> Result<Option<&'a IndexedNode>, DctxError> {
    if target.is_empty() {
        return Err(DctxError::invalid("target is required"));
    }
    Ok(lookup(nodes, text, target))
}

fn require_index(nodes: &[IndexedNode], text: &TextBlob, target: &str) -> Result<Option<u32>, DctxError> {
    if target.is_empty() {
        return Err(DctxError::invalid("target is required"));
    }
    let Some(index) = lookup_index(nodes, text, target) else {
        return Ok(None);
    };
    Ok(u32::try_from(index).ok())
}

pub(super) fn missing(target: &str) -> Collected {
    Collected::Outcome(IdentityOutcome::Gone { id: target.to_string() })
}

fn admit(text: &TextBlob, node: &IndexedNode, request: &MapRequest) -> bool {
    if !request.kinds.is_empty() && !request.kinds.iter().any(|kind| kind == &node.kind || (kind == "doc" && node.letter == b'd')) {
        return false;
    }
    match &request.within {
        Some(prefix) => {
            let path = text.get(node.path);
            path == prefix || path.strip_prefix(prefix.as_str()).is_some_and(|rest| rest.starts_with('/'))
        }
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

pub(super) fn record_of(text: &TextBlob, node: &IndexedNode) -> AnswerRecord {
    let mut id = String::new();
    write_node_id(&mut id, text, node);
    // Files read by a heuristic family are labelled; lexed languages leave the field to neighbors.
    let confidence = generic::handles(language_of(text.get(node.path))).then(|| generic::GUESS.to_string());
    AnswerRecord {
        id,
        kind: node.kind.clone(),
        path: text.get(node.path).to_string(),
        lines: node.lines,
        summary: node.summary.clone(),
        summary_source: node.summary_source.clone(),
        signature: None,
        span: None,
        source_utf8: None,
        edge: None,
        direction: None,
        target: None,
        confidence,
        score: 0,
    }
}

fn score_of(text: &TextBlob, node: &IndexedNode, query: &str) -> i64 {
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
    if text.get(node.path).contains(query) {
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
        let mut text = TextBlob::default();
        let path = text.push("src/lib.rs");
        let node = IndexedNode {
            letter: b's',
            ordinal: 0,
            kind: "fn".to_string(),
            name: "ping".to_string(),
            path,
            lines: [2, 2],
            span: [13, 29],
            summary: "Respond.".to_string(),
            summary_source: "doc".to_string(),
            signature: "pub fn ping()".to_string(),
        };
        assert_eq!(score_of(&text, &node, "ping"), 3000);
        assert_eq!(score_of(&text, &node, "src/lib"), 100);
        assert_eq!(score_of(&text, &node, "Respond"), 10);
        assert_eq!(score_of(&text, &node, "missing"), 0);
        let qualified = IndexedNode { name: "tests::ping".to_string(), ..node };
        assert_eq!(score_of(&text, &qualified, "ping"), 2500);
    }

    #[test]
    fn records_of_heuristic_languages_are_labelled_guess() {
        let mut text = TextBlob::default();
        let scala = text.push("src/Main.scala");
        let python = text.push("src/main.py");
        let node = |path| IndexedNode {
            letter: b's',
            ordinal: 0,
            kind: "fn".to_string(),
            name: "run".to_string(),
            path,
            lines: [1, 1],
            span: [0, 0],
            summary: String::new(),
            summary_source: "sig".to_string(),
            signature: String::new(),
        };
        assert_eq!(record_of(&text, &node(scala)).confidence.as_deref(), Some("guess"));
        assert_eq!(record_of(&text, &node(python)).confidence, None);
    }
}

#[cfg(test)]
#[path = "neighbors_tests.rs"]
mod neighbors_tests;
