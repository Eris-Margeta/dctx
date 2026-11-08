// SPDX-License-Identifier: Apache-2.0
//! Ranking and selection for the five verbs. Page fitting stays with the session.
//! Implements contracts/API-PORTS-v1.md record order: score descending, id ascending.
//! Depends on the in-memory generation. Map, find and neighbors do not read the source.
//! A missing id is [`Collected::Outcome`], not an empty success.

use super::concept;
use super::extract::language_of;
use super::outline_order;
use super::{Generation, IndexedNode, TextBlob, generic, write_node_id};
use crate::domain::{AnswerRecord, DctxError, Direction, IdentityOutcome, MapRequest, Verb};

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
pub(crate) fn collect(
    generation: &Generation,
    request: &MapRequest,
    depth: u32,
) -> Result<Collected, DctxError> {
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
pub(crate) fn lookup<'a>(generation: &'a Generation, target: &str) -> Option<&'a IndexedNode> {
    generation.nodes.get(generation.node_index(target)?)
}

fn node_at(nodes: &[IndexedNode], index: u32) -> Option<&IndexedNode> {
    nodes.get(usize::try_from(index).unwrap_or(usize::MAX))
}

fn map_children(
    generation: &Generation,
    request: &MapRequest,
    depth: u32,
) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(root) = require_index(generation, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut frontier = vec![root];
    let mut children = Vec::new();
    for level in 1..=depth {
        let mut next = Vec::new();
        for id in &frontier {
            for edge in &generation.edges {
                if edge.src == *id && text.get(edge.edge) == "contains" {
                    next.push(edge.dst);
                }
            }
        }
        children.extend(next.iter().map(|id| (*id, level)));
        frontier = next;
    }
    // A file root lists its outline by tier and source line (see `outline_order`); any other root keeps the id order.
    let file_root = node_at(&generation.nodes, root).is_some_and(|node| node.kind == "module");
    let mut ranked = Vec::new();
    for (id, level) in children {
        if let Some(node) = node_at(&generation.nodes, id)
            && admit(text, node, request)
        {
            let tier = if file_root {
                outline_order::tier(node, text.get(node.path), level)
            } else {
                0
            };
            ranked.push((tier, record_of(text, node)));
        }
    }
    if file_root {
        ranked.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.lines[0].cmp(&right.1.lines[0]))
                .then(left.1.id.cmp(&right.1.id))
        });
    } else {
        ranked.sort_by(|left, right| left.1.id.cmp(&right.1.id));
    }
    Ok(Collected::Records(
        ranked.into_iter().map(|(_, record)| record).collect(),
    ))
}

fn find(generation: &Generation, request: &MapRequest) -> Vec<AnswerRecord> {
    let text = &generation.text;
    let mut records = Vec::new();
    let terms = concept::terms(&request.query);
    for node in &generation.nodes {
        if node.kind == "module" || !admit(text, node, request) {
            continue;
        }
        let mut score = score_of(text, node, &request.query).max(
            concept::score(&terms, &node.name, &node.summary, text.get(node.path)).unwrap_or(0),
        );
        if !terms.is_empty() && concept::phrase(&request.query, &node.name) {
            score = score.max(concept::PHRASE);
        }
        if score == 0 {
            continue;
        }
        // A key of a data file (JSON, YAML, TOML) that matches by name ranks below a code symbol of the same name, even a qualified one.
        if score >= 2500 && language_of(text.get(node.path)) == "config" {
            score -= DATA_KEY_PENALTY;
        }
        let mut record = record_of(text, node);
        record.score = score;
        records.push(record);
    }
    records.sort_by(|left, right| right.score.cmp(&left.score).then(left.id.cmp(&right.id)));
    records
}

/// Score taken off a name match on a data-file key, so `2400` and `1900` sit below code's `3000` and `2500` and above a substring match.
const DATA_KEY_PENALTY: i64 = 600;

/// Breadth-first neighbors. Each node is listed once, at its smallest depth, levels in order, each level by edge, target and id.
/// A line deeper than one carries its depth after the direction, as `calls:in:2:<id>`; depth-1 lines keep `calls:in:<id>`.
fn neighbors(
    generation: &Generation,
    request: &MapRequest,
    depth: u32,
) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(root) = require_index(generation, &request.target)? else {
        return Ok(missing(&request.target));
    };
    // A declaration-only symbol answers with its paired definition's records, exactly as if asked for the definition.
    let root = super::decl_pair::paired_definition(generation, root).unwrap_or(root);
    let mut frontier = vec![root];
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut records = Vec::new();
    for level in 1..=depth {
        let mut found = Vec::new();
        let mut next: Vec<u32> = Vec::new();
        let mut queued: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for id in &frontier {
            for edge in &generation.edges {
                let (direction, other) =
                    if edge.src == *id && direction_ok(request.direction, "out") {
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
                if !request.edges.is_empty()
                    && !request.edges.iter().any(|kind| kind.as_str() == edge_name)
                {
                    continue;
                }
                let Some(node) = node_at(&generation.nodes, other) else {
                    continue;
                };
                if !admit(text, node, request) {
                    continue;
                }
                let mut record = record_of(text, node);
                clip_summary(&mut record);
                let node_id = record.id.clone();
                record.id = if level > 1 {
                    format!("{edge_name}:{direction}:{level}:{node_id}")
                } else {
                    format!("{edge_name}:{direction}:{node_id}")
                };
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
        found.sort_by(|left, right| {
            left.edge
                .cmp(&right.edge)
                .then(left.target.cmp(&right.target))
                .then(left.id.cmp(&right.id))
        });
        found.dedup_by(|left, right| left.id == right.id);
        records.append(&mut spread_by_file(found));
        // The root stays listable at depth one (a self call); from depth two on it and every earlier node are settled.
        seen.insert(root);
        seen.extend(next.iter().copied());
        frontier = next;
    }
    Ok(Collected::Records(records))
}

/// Reorders one level, sorted by edge, so within each edge the first record of every (direction, file) comes before the repeats.
/// A first page then names every calling file before it lists more callers of one file. Both runs keep the sorted order.
fn spread_by_file(found: Vec<AnswerRecord>) -> Vec<AnswerRecord> {
    let mut seen = std::collections::HashSet::new();
    let mut keyed: Vec<(bool, AnswerRecord)> = found
        .into_iter()
        .map(|record| {
            (
                !seen.insert((
                    record.edge.clone(),
                    record.direction.clone(),
                    record.path.clone(),
                )),
                record,
            )
        })
        .collect();
    keyed.sort_by(|left, right| left.1.edge.cmp(&right.1.edge).then(left.0.cmp(&right.0)));
    keyed.into_iter().map(|(_, record)| record).collect()
}

/// Longest summary kept on an edge record, in characters.
const EDGE_SUMMARY_CAP: usize = 100;

/// Keeps an edge record small: a summary taken from the signature is dropped, a doc summary is clipped on a char boundary.
fn clip_summary(record: &mut AnswerRecord) {
    if record.summary_source == "sig" {
        record.summary.clear();
    } else if let Some((cut, _)) = record.summary.char_indices().nth(EDGE_SUMMARY_CAP) {
        record.summary.truncate(cut);
    }
}

fn explain(generation: &Generation, request: &MapRequest) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(node) = require(generation, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut record = record_of(text, node);
    if node.letter != b'm' && !node.signature.is_empty() {
        record.signature = Some(node.signature.to_string());
    }
    Ok(Collected::Records(vec![record]))
}

fn read(generation: &Generation, request: &MapRequest) -> Result<Collected, DctxError> {
    let text = &generation.text;
    let Some(node) = require(generation, &request.target)? else {
        return Ok(missing(&request.target));
    };
    let mut record = record_of(text, node);
    record.span = Some(node.span);
    Ok(Collected::Read(Box::new(record)))
}

pub(super) fn require<'a>(
    generation: &'a Generation,
    target: &str,
) -> Result<Option<&'a IndexedNode>, DctxError> {
    if target.is_empty() {
        return Err(DctxError::invalid("target is required"));
    }
    Ok(lookup(generation, target))
}

fn require_index(generation: &Generation, target: &str) -> Result<Option<u32>, DctxError> {
    if target.is_empty() {
        return Err(DctxError::invalid("target is required"));
    }
    let Some(index) = generation.node_index(target) else {
        return Ok(None);
    };
    Ok(u32::try_from(index).ok())
}

pub(super) fn missing(target: &str) -> Collected {
    Collected::Outcome(IdentityOutcome::Gone {
        id: target.to_string(),
    })
}

fn admit(text: &TextBlob, node: &IndexedNode, request: &MapRequest) -> bool {
    if !request.kinds.is_empty()
        && !request
            .kinds
            .iter()
            .any(|kind| node.kind == *kind || (kind == "doc" && node.letter == b'd'))
    {
        return false;
    }
    match &request.within {
        Some(prefix) => {
            let path = text.get(node.path);
            path == prefix
                || path
                    .strip_prefix(prefix.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
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
    let confidence =
        generic::handles(language_of(text.get(node.path))).then(|| generic::GUESS.to_string());
    AnswerRecord {
        id,
        kind: node.kind.to_string(),
        path: text.get(node.path).to_string(),
        lines: node.lines,
        summary: node.summary.to_string(),
        summary_source: node.summary_source.to_string(),
        signature: None,
        span: None,
        source_utf8: None,
        edge: None,
        direction: None,
        target: None,
        confidence,
        sites: Vec::new(),
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
    // A qualified name matches on its last part: `Type::name` for symbols, `table.key` for configuration keys.
    if node
        .name
        .strip_suffix(query)
        .is_some_and(|head| head.ends_with("::") || head.ends_with('.'))
    {
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
            kind: "fn".to_string().into(),
            name: "ping".to_string().into(),
            path,
            lines: [2, 2],
            span: [13, 29],
            summary: "Respond.".to_string().into(),
            summary_source: "doc".to_string().into(),
            signature: "pub fn ping()".to_string().into(),
        };
        assert_eq!(score_of(&text, &node, "ping"), 3000);
        assert_eq!(score_of(&text, &node, "src/lib"), 100);
        assert_eq!(score_of(&text, &node, "Respond"), 10);
        assert_eq!(score_of(&text, &node, "missing"), 0);
        let qualified = IndexedNode {
            name: "tests::ping".to_string().into(),
            ..node
        };
        assert_eq!(score_of(&text, &qualified, "ping"), 2500);
        let key = IndexedNode {
            name: "server.max_upload_size".to_string().into(),
            ..qualified
        };
        assert_eq!(score_of(&text, &key, "max_upload_size"), 2500);
        assert_eq!(score_of(&text, &key, "upload"), 1000);
    }

    #[test]
    fn records_of_heuristic_languages_are_labelled_guess() {
        let mut text = TextBlob::default();
        let scala = text.push("src/Main.scala");
        let python = text.push("src/main.py");
        let infra = text.push("deploy/Makefile");
        let node = |path| IndexedNode {
            letter: b's',
            ordinal: 0,
            kind: "fn".to_string().into(),
            name: "run".to_string().into(),
            path,
            lines: [1, 1],
            span: [0, 0],
            summary: String::new().into(),
            summary_source: "sig".to_string().into(),
            signature: String::new().into(),
        };
        assert_eq!(
            record_of(&text, &node(scala)).confidence.as_deref(),
            Some("guess")
        );
        assert_eq!(
            record_of(&text, &node(infra)).confidence.as_deref(),
            Some("guess")
        );
        assert_eq!(record_of(&text, &node(python)).confidence, None);
    }
}

#[cfg(test)]
#[path = "neighbors_tests.rs"]
mod neighbors_tests;
