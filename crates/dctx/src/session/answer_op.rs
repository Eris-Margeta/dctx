// SPDX-License-Identifier: Apache-2.0
//! Bounded directed answers over the authorized graph of one pinned generation.

use std::collections::{BTreeMap, VecDeque};

use super::{Session, SnapshotHandle};
use crate::index::{Generation, impact, write_node_id};
use crate::{DctxError, MapRequest, Verb};

/// Source location and enclosing symbol in a graph answer.
#[derive(Clone, Debug)]
pub struct AnswerLocation {
    /// Stable node id.
    pub id: String,
    /// Place-relative file.
    pub file: String,
    /// One-based source line.
    pub line: u32,
    /// Enclosing symbol name (or module path).
    pub symbol: String,
    /// Distance from the requested symbol; zero for trace locations.
    pub depth: usize,
}

/// Complete shortest paths within the requested bounds.
#[derive(Debug)]
pub struct TraceAnswer {
    /// Paths ordered lexically by node locations.
    pub paths: Vec<Vec<AnswerLocation>>,
    /// The depth or path count bound prevented a complete answer.
    pub truncated: bool,
}

/// Direct sites followed by distinct transitive callers.
#[derive(Debug)]
pub struct ImpactAnswer {
    /// Every retained direct call/reference line, in file/line order.
    pub direct: Vec<AnswerLocation>,
    /// Distinct callers at depth two or greater.
    pub callers: Vec<AnswerLocation>,
    /// A depth or record bound prevented a complete answer.
    pub truncated: bool,
}

struct Graph {
    locations: Vec<AnswerLocation>,
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
    allowed: Vec<bool>,
}

impl Graph {
    fn new(generation: &Generation, allowed: Vec<bool>) -> Self {
        let locations: Vec<_> = generation
            .nodes
            .iter()
            .map(|node| {
                let mut id = String::new();
                write_node_id(&mut id, &generation.text, node);
                let file = generation.text.get(node.path).to_string();
                AnswerLocation {
                    id,
                    symbol: if node.name.is_empty() {
                        file.clone()
                    } else {
                        node.name.to_string()
                    },
                    file,
                    line: node.lines[0],
                    depth: 0,
                }
            })
            .collect();
        let mut outgoing = vec![Vec::new(); locations.len()];
        let mut incoming = outgoing.clone();
        for edge in &generation.edges {
            let (src, dst) = (edge.src as usize, edge.dst as usize);
            if !matches!(
                generation.text.get(edge.edge),
                "calls" | "imports" | "uses-type" | "references"
            ) || !allowed.get(src).copied().unwrap_or(false)
                || !allowed.get(dst).copied().unwrap_or(false)
            {
                continue;
            }
            outgoing[src].push(dst);
            incoming[dst].push(src);
        }
        for neighbors in outgoing.iter_mut().chain(incoming.iter_mut()) {
            neighbors
                .sort_by_key(|&at| (&locations[at].file, locations[at].line, &locations[at].id));
            neighbors.dedup();
        }
        Self {
            locations,
            outgoing,
            incoming,
            allowed,
        }
    }

    fn resolve(&self, target: &str) -> Result<Option<usize>, DctxError> {
        let exact = target.contains(':') || target.starts_with('@');
        let matches: Vec<_> = self
            .locations
            .iter()
            .enumerate()
            .filter(|(at, loc)| {
                self.allowed[*at] && (loc.id == target || !exact && loc.symbol == target)
            })
            .map(|(at, _)| at)
            .take(2)
            .collect();
        match matches.as_slice() {
            [] => Ok(None),
            [at] => Ok(Some(*at)),
            _ => Err(DctxError::invalid_detail(
                "ambiguous symbol; use a stable id",
                target,
            )),
        }
    }
}

impl Session {
    fn answer_graph(&self, generation: &Generation) -> Result<Graph, DctxError> {
        let mut files = BTreeMap::new();
        for node in &generation.nodes {
            self.cancel.checkpoint().map_err(DctxError::from_core)?;
            let path = generation.text.get(node.path);
            files
                .entry(path)
                .or_insert_with(|| self.authorize(path).is_ok());
        }
        Ok(Graph::new(
            generation,
            generation
                .nodes
                .iter()
                .map(|node| files[generation.text.get(node.path)])
                .collect(),
        ))
    }

    /// Returns shortest directed call/reference paths. Names must resolve uniquely.
    /// Depth is 1..=16 and the maximum number of paths is 1..=100.
    /// Missing endpoints yield an empty answer so transports can append outside-index notices.
    /// # Errors
    /// Refuses invalid bounds, handles, cancellation, or ambiguous names.
    pub fn trace_paths(
        &mut self,
        handle: &SnapshotHandle,
        from: &str,
        to: &str,
        depth: usize,
        max_paths: usize,
    ) -> Result<TraceAnswer, DctxError> {
        answer_bound("trace", "--max-depth", depth, 16)?;
        answer_bound("trace", "--max-paths", max_paths, 100)?;
        self.reject_transport(&MapRequest::new(Verb::Neighbors))?;
        let generation = self.generation(handle)?;
        let graph = self.answer_graph(&generation)?;
        let (Some(start), Some(goal)) = (graph.resolve(from)?, graph.resolve(to)?) else {
            return Ok(TraceAnswer {
                paths: Vec::new(),
                truncated: false,
            });
        };
        let (dist, cut) = distances(&graph.incoming, goal, depth);
        if dist[start] == usize::MAX {
            return Ok(TraceAnswer {
                paths: Vec::new(),
                truncated: cut,
            });
        }
        let mut paths = Vec::new();
        collect_paths(
            &graph,
            &dist,
            &mut vec![start],
            goal,
            max_paths + 1,
            &mut paths,
        );
        let truncated = paths.len() > max_paths;
        paths.truncate(max_paths);
        Ok(TraceAnswer { paths, truncated })
    }

    /// Returns direct reference/call lines and distinct transitive callers.
    /// Reads authorized caller files through the verified source path. Results are
    /// bounded to 1..=10000 records and depth 1..=16, with direct sites admitted first.
    /// # Errors
    /// Refuses invalid bounds, handles, cancellation, ambiguous names, or changed source.
    pub fn symbol_impact(
        &mut self,
        handle: &SnapshotHandle,
        target: &str,
        depth: usize,
        max_records: usize,
    ) -> Result<ImpactAnswer, DctxError> {
        answer_bound("impact", "--max-depth", depth, 16)?;
        answer_bound("impact", "--max-sites", max_records, 10000)?;
        self.reject_transport(&MapRequest::new(Verb::Neighbors))?;
        let generation = self.generation(handle)?;
        let mut graph = self.answer_graph(&generation)?;
        let mut answer = ImpactAnswer {
            direct: Vec::new(),
            callers: Vec::new(),
            truncated: false,
        };
        let root = match graph.resolve(target)? {
            Some(root) => root,
            None => {
                // A definition outside this index can still have indexed references.
                // A virtual root lets those sites seed the same caller traversal.
                graph.incoming.push(Vec::new());
                graph.locations.len()
            }
        };
        let name = graph
            .locations
            .get(root)
            .map_or(target, |loc| loc.symbol.as_str())
            .rsplit(['.', ':', '#'])
            .next()
            .unwrap_or(target)
            .split('~')
            .next()
            .unwrap_or(target)
            .to_string();
        let mut files: Vec<_> = generation
            .files
            .iter()
            .filter(|file| file.digest.is_some())
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let mut by_file = BTreeMap::<_, Vec<_>>::new();
        for (at, node) in generation.nodes.iter().enumerate() {
            if graph.allowed[at] {
                by_file
                    .entry(graph.locations[at].file.as_str())
                    .or_default()
                    .push((at, node));
            }
        }
        for file in files {
            self.cancel.checkpoint().map_err(DctxError::from_core)?;
            if self.authorize(&file.path).is_err() {
                continue;
            }
            let Some(digest) = file.digest else {
                continue;
            };
            let bytes = self.read_verified(&file.path, digest)?;
            let Ok(source) = std::str::from_utf8(&bytes) else {
                continue;
            };
            let Some(nodes) = by_file.get(file.path.as_str()) else {
                continue;
            };
            let mut seen = std::collections::BTreeSet::new();
            impact::references(source, &file.path, &name, |offset, line| {
                let Some(&(at, node)) = nodes
                    .iter()
                    .filter(|(_, n)| n.span[0] <= offset as u64 && (offset as u64) < n.span[1])
                    .min_by_key(|(_, n)| n.span[1] - n.span[0])
                else {
                    return true;
                };
                // A declaration's own name is not a reference; later occurrences on
                // the same line (including inline recursion) still count.
                if node.lines[0] == line && node.name.rsplit(['.', ':', '#']).next() == Some(&name)
                {
                    let start = usize::try_from(node.span[0]).unwrap_or(0);
                    let first = source
                        .get(start..)
                        .and_then(|s| s.find(&name))
                        .map(|pos| start + pos);
                    if first == Some(offset) {
                        return true;
                    }
                }
                if !seen.insert((at, line)) {
                    return true;
                }
                if answer.direct.len() == max_records {
                    answer.truncated = true;
                    return false;
                }
                let mut loc = graph.locations[at].clone();
                loc.line = line;
                loc.depth = 1;
                answer.direct.push(loc);
                if at != root {
                    graph.incoming[root].push(at);
                }
                true
            });
            if answer.truncated {
                break;
            }
        }
        let (dist, cut) = distances(&graph.incoming, root, depth);
        answer.truncated |= cut;
        let mut callers: Vec<_> = dist
            .iter()
            .enumerate()
            .filter(|(_, d)| **d >= 2 && **d != usize::MAX)
            .map(|(at, d)| {
                let mut loc = graph.locations[at].clone();
                loc.depth = *d;
                loc
            })
            .collect();
        callers.sort_by(|a, b| (&a.file, a.line, &a.id).cmp(&(&b.file, b.line, &b.id)));
        let left = max_records.saturating_sub(answer.direct.len());
        answer.truncated |= callers.len() > left;
        callers.truncate(left);
        answer.callers = callers;
        answer
            .direct
            .sort_by(|a, b| (&a.file, a.line, &a.id).cmp(&(&b.file, b.line, &b.id)));
        Ok(answer)
    }
}

fn answer_bound(verb: &str, option: &str, value: usize, max: usize) -> Result<(), DctxError> {
    if (1..=max).contains(&value) {
        Ok(())
    } else {
        Err(DctxError::invalid_detail(
            "answer bounds",
            &format!("{option} {value} for {verb} (allowed range 1..={max})"),
        ))
    }
}

fn distances(edges: &[Vec<usize>], root: usize, max_depth: usize) -> (Vec<usize>, bool) {
    let mut dist = vec![usize::MAX; edges.len()];
    let mut queue = VecDeque::from([root]);
    dist[root] = 0;
    let mut cut = false;
    while let Some(at) = queue.pop_front() {
        for &next in &edges[at] {
            if dist[next] != usize::MAX {
                continue;
            }
            if dist[at] == max_depth {
                cut = true;
                continue;
            }
            dist[next] = dist[at] + 1;
            queue.push_back(next);
        }
    }
    (dist, cut)
}

fn collect_paths(
    graph: &Graph,
    dist: &[usize],
    path: &mut Vec<usize>,
    goal: usize,
    cap: usize,
    out: &mut Vec<Vec<AnswerLocation>>,
) {
    let at = *path.last().unwrap_or(&goal);
    if at == goal {
        out.push(
            path.iter()
                .map(|&index| graph.locations[index].clone())
                .collect(),
        );
        return;
    }
    for &next in &graph.outgoing[at] {
        if dist[next].checked_add(1) != Some(dist[at]) {
            continue;
        }
        path.push(next);
        collect_paths(graph, dist, path, goal, cap, out);
        path.pop();
        if out.len() == cap {
            break;
        }
    }
}
