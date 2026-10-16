//! Bounded graph hydration for persistent map, neighbors, explain and read queries.
//! Implements SPEC-MAP-MODE section 4 against the reader's pinned WAL snapshot.
//! Loads requested roots and reachable edges while retaining global degrees.
use super::Index;
use crate::{Direction, Error, Request, Result, Verb, domain::validate_path};
use dctx_map_model::{Edge, IdReference, Node, StableId};
use rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeSet;

impl Index {
    pub(super) fn graph_view(&self, request: &Request) -> Result<Self> {
        let reader = self
            .reader
            .as_ref()
            .ok_or_else(|| Error::Request("missing snapshot".into()))?;
        let mut view = Self::with_store(self.place.clone(), None);
        view.files = self.files.clone();
        view.snapshot = self.snapshot.clone();
        view.report = self.report.clone();
        view.moves = self.moves.clone();
        view.ready = self.ready;
        let roots = if request.verb == Verb::Map
            && (request.target.is_empty()
                || !request.target.contains(':') && !request.target.starts_with('@'))
        {
            path_roots(reader, &request.target)?
        } else {
            identity_roots(reader, &self.moves, &request.target)?
        };
        let depth = match request.verb {
            Verb::Read => 0,
            Verb::Explain => 1,
            _ => request.depth,
        };
        let mut frontier = roots;
        let mut seen = BTreeSet::new();
        for level in 0..=depth {
            let mut next = Vec::new();
            for id in frontier {
                if !seen.insert(id.clone()) {
                    continue;
                }
                load_node(reader, &id, &mut view)?;
                if level == depth {
                    continue;
                }
                let sql = if request.verb == Verb::Map {
                    "SELECT payload FROM query_edges WHERE src=?1 AND kind='contains'"
                } else {
                    "SELECT payload FROM query_edges WHERE src=?1 OR dst=?1"
                };
                let mut edges = reader.prepare_cached(sql)?;
                for row in edges.query_map([id.as_str()], |r| r.get::<_, String>(0))? {
                    let edge: Edge = serde_json::from_str(&row?)?;
                    if request.verb == Verb::Neighbors {
                        let incoming = edge.dst == id && edge.src != id;
                        let outgoing = edge.src == id && edge.dst != id;
                        if incoming && request.direction == Direction::Out
                            || outgoing && request.direction == Direction::In
                            || !request.edges.is_empty() && !request.edges.contains(&edge.kind)
                        {
                            continue;
                        }
                    }
                    let other = if edge.src == id { &edge.dst } else { &edge.src };
                    next.push(other.clone());
                    view.edges.push(edge);
                }
            }
            frontier = next;
        }
        view.edges.sort_by(|a, b| {
            (&a.owner_path, &a.src, &a.dst, a.kind, a.conf).cmp(&(
                &b.owner_path,
                &b.src,
                &b.dst,
                b.kind,
                b.conf,
            ))
        });
        view.edges.dedup();
        let degree = std::mem::take(&mut view.degree);
        view.adjacency();
        view.degree = degree;
        Ok(view)
    }
}

fn load_node(reader: &Connection, id: &StableId, view: &mut Index) -> Result<()> {
    if view.nodes.contains_key(id) {
        return Ok(());
    }
    let row = reader
        .query_row(
            "SELECT payload,degree FROM query_nodes WHERE id=?1",
            [id.as_str()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, usize>(1)?)),
        )
        .optional()?;
    if let Some((payload, degree)) = row {
        let node: Node = serde_json::from_str(&payload)?;
        view.degree.insert(node.id.clone(), degree);
        view.nodes.insert(node.id.clone(), node);
    }
    Ok(())
}

fn path_roots(reader: &Connection, target: &str) -> Result<Vec<StableId>> {
    let path = if target.is_empty() {
        "."
    } else {
        target.trim_end_matches('/')
    };
    validate_path(path)?;
    if path == "." {
        return Ok(vec!["p:.".parse()?]);
    }
    let mut exact = reader.prepare_cached(
        "SELECT id FROM query_nodes WHERE path=?1 AND kind IN ('package','module') ORDER BY id LIMIT 1")?;
    let id: Option<String> = exact.query_row([path], |r| r.get(0)).optional()?;
    if let Some(id) = id {
        return Ok(vec![id.parse()?]);
    }
    let mut statement = reader.prepare_cached(
        "SELECT id FROM query_nodes WHERE kind='module' AND (path=?1 OR substr(path,1,length(?1)+1)=?1||'/') ORDER BY id")?;
    statement
        .query_map([path], |r| r.get::<_, String>(0))?
        .map(|row| Ok(row?.parse()?))
        .collect()
}

fn identity_roots(
    reader: &Connection,
    moves: &std::collections::BTreeMap<StableId, StableId>,
    target: &str,
) -> Result<Vec<StableId>> {
    let mut id = match target.parse::<IdReference>()? {
        IdReference::Full(id) => id,
        IdReference::Alias(alias) => {
            let mut statement =
                reader.prepare_cached("SELECT id FROM query_nodes WHERE alias=?1")?;
            let mut ids: BTreeSet<StableId> = statement
                .query_map([&alias], |r| r.get::<_, String>(0))?
                .map(|row| Ok(row?.parse()?))
                .collect::<Result<_>>()?;
            ids.extend(
                moves
                    .keys()
                    .filter(|id| id.alias().as_deref() == Some(&alias))
                    .cloned(),
            );
            if ids.len() != 1 {
                return Err(Error::Request(format!(
                    "unknown or ambiguous alias {alias}; try: dctx map find {alias}"
                )));
            }
            ids.into_iter()
                .next()
                .ok_or_else(|| Error::Request("alias disappeared".into()))?
        }
    };
    let mut seen = BTreeSet::new();
    while seen.insert(id.clone()) {
        let exists: bool = reader.query_row(
            "SELECT EXISTS(SELECT 1 FROM query_nodes WHERE id=?1)",
            [id.as_str()],
            |r| r.get(0),
        )?;
        if exists {
            break;
        }
        let Some(next) = moves.get(&id) else {
            break;
        };
        id = next.clone();
    }
    Ok(vec![id])
}
