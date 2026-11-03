// SPDX-License-Identifier: Apache-2.0
//! Hubs: the most-called symbols of a generation and per-directory counts (the `dctx map hubs` verb).
//! Counts incoming `calls` and `imports` edges per symbol node. It is a view over the edges the walk already stored.
//! An import whose file also calls the same symbol is the same dependency, so it is counted once, as the calls.
//! The page is its own text or JSON shape: it is not an [`crate::domain::Answer`] and has no cursor.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use super::{write_node_id, Generation};

/// One symbol with its incoming calls and imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hub {
    /// Stable id of the symbol.
    pub id: String,
    /// Wire kind (`fn`, `struct`, ...).
    pub kind: String,
    /// Root-relative path of the file that declares it.
    pub path: String,
    /// Incoming calls plus imports that no call from the same file already covers.
    pub in_degree: u32,
}

/// Counts for one top-level directory. Files in the place root are under `./`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirCount {
    /// Directory with a trailing slash, or `./`.
    pub dir: String,
    /// Indexed files.
    pub files: u32,
    /// Indexed symbols.
    pub symbols: u32,
}

/// Header fields of a hubs page. `age_s` and the snapshot fields appear only with `meta`.
#[derive(Debug, Clone)]
pub(crate) struct PageHead<'a> {
    /// Place copied into the page.
    pub place: &'a str,
    /// Own snapshot digest.
    pub snapshot: &'a str,
    /// Generation id.
    pub generation: u64,
    /// Freshness mode label.
    pub mode: &'a str,
    /// Freshness state label.
    pub state: &'a str,
    /// Seconds since the snapshot.
    pub age_s: u64,
    /// Coverage label.
    pub coverage: &'a str,
    /// Completion label.
    pub completion: &'a str,
    /// Parser id.
    pub parser: &'a str,
    /// Restores the snapshot digest, generation and age.
    pub meta: bool,
}

/// Every symbol with an incoming edge, by in-degree descending then id, and every top-level directory by name.
pub(crate) fn collect_hubs(generation: &Generation) -> (Vec<Hub>, Vec<DirCount>) {
    let text = &generation.text;
    let nodes = &generation.nodes;
    let node_path = |index: u32| nodes.get(usize::try_from(index).unwrap_or(usize::MAX)).map_or("", |node| text.get(node.path));
    let mut called: HashSet<(u32, &str)> = HashSet::new();
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for pass in ["calls", "imports"] {
        for edge in generation.edges.iter().filter(|edge| text.get(edge.edge) == pass) {
            let is_symbol = nodes.get(usize::try_from(edge.dst).unwrap_or(usize::MAX)).is_some_and(|node| node.letter == b's');
            if edge.src == edge.dst || !is_symbol {
                continue;
            }
            if pass == "calls" {
                called.insert((edge.dst, node_path(edge.src)));
            } else if called.contains(&(edge.dst, node_path(edge.src))) {
                continue;
            }
            *counts.entry(edge.dst).or_insert(0) += 1;
        }
    }
    let mut hubs: Vec<Hub> = counts
        .into_iter()
        .filter_map(|(index, in_degree)| {
            let node = nodes.get(usize::try_from(index).ok()?)?;
            let mut id = String::new();
            write_node_id(&mut id, text, node);
            Some(Hub { id, kind: node.kind.clone(), path: text.get(node.path).to_string(), in_degree })
        })
        .collect();
    hubs.sort_by(|left, right| right.in_degree.cmp(&left.in_degree).then_with(|| left.id.cmp(&right.id)));
    let mut dirs: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for node in nodes {
        let slot = match node.letter {
            b'm' => 0,
            b's' => 1,
            _ => continue,
        };
        let path = text.get(node.path);
        let dir = path.split_once('/').map_or_else(|| "./".to_string(), |(top, _)| format!("{top}/"));
        let entry = dirs.entry(dir).or_insert((0, 0));
        if slot == 0 {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    let dirs = dirs.into_iter().map(|(dir, (files, symbols))| DirCount { dir, files, symbols }).collect();
    (hubs, dirs)
}

/// Encodes one page. `json` selects the JSON v2 style object, otherwise the text page.
pub(crate) fn encode(head: &PageHead<'_>, hubs: &[Hub], dirs: &[DirCount], json: bool) -> String {
    if json { encode_json(head, hubs, dirs) } else { encode_text(head, hubs, dirs) }
}

/// Text page: `# <place> hubs`, one tab line per hub, then one line per directory.
fn encode_text(head: &PageHead<'_>, hubs: &[Hub], dirs: &[DirCount]) -> String {
    let mut out = format!("# {} hubs", head.place);
    if head.meta {
        let _ = write!(out, " {} g={}", head.snapshot, head.generation);
    }
    out.push('\n');
    for hub in hubs {
        let _ = writeln!(out, "{}\t{}\t{}\t{}", hub.id, hub.kind, hub.path, hub.in_degree);
    }
    for dir in dirs {
        let _ = writeln!(out, "{}\t{} files\t{} symbols", dir.dir, dir.files, dir.symbols);
    }
    out
}

/// JSON page with the same envelope as the other verbs, `records` of hubs and a `dirs` array.
fn encode_json(head: &PageHead<'_>, hubs: &[Hub], dirs: &[DirCount]) -> String {
    let mut out = String::from("{\"v\":2,\"place\":");
    quote(&mut out, head.place);
    if head.meta {
        out.push_str(",\"snapshot\":");
        quote(&mut out, head.snapshot);
        let _ = write!(out, ",\"generation\":{}", head.generation);
    }
    out.push_str(",\"verb\":\"hubs\",\"freshness\":{\"mode\":");
    quote(&mut out, head.mode);
    out.push_str(",\"state\":");
    quote(&mut out, head.state);
    if head.meta {
        let _ = write!(out, ",\"age_s\":{}", head.age_s);
    }
    out.push_str("},\"precision\":{\"coverage\":");
    quote(&mut out, head.coverage);
    out.push_str(",\"completion\":");
    quote(&mut out, head.completion);
    out.push_str(",\"parser\":");
    quote(&mut out, head.parser);
    out.push_str("},\"records\":[");
    for (index, hub) in hubs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        quote(&mut out, &hub.id);
        out.push_str(",\"kind\":");
        quote(&mut out, &hub.kind);
        out.push_str(",\"path\":");
        quote(&mut out, &hub.path);
        let _ = write!(out, ",\"in_degree\":{}}}", hub.in_degree);
    }
    out.push_str("],\"dirs\":[");
    for (index, dir) in dirs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"dir\":");
        quote(&mut out, &dir.dir);
        let _ = write!(out, ",\"files\":{},\"symbols\":{}}}", dir.files, dir.symbols);
    }
    out.push_str("]}\n");
    out
}

/// Appends `text` as a JSON string.
pub(crate) fn quote(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FreshnessMode;
    use crate::index::{IndexedEdge, IndexedNode, TextBlob};

    fn node(text: &mut TextBlob, letter: u8, kind: &str, name: &str, path: &str) -> IndexedNode {
        IndexedNode {
            letter,
            ordinal: 0,
            kind: kind.to_string(),
            name: name.to_string(),
            path: text.push(path),
            lines: [1, 1],
            span: [0, 0],
            summary: String::new(),
            summary_source: String::new(),
            signature: String::new(),
        }
    }

    fn fixture() -> Generation {
        let mut text = TextBlob::default();
        let nodes = vec![
            node(&mut text, b'm', "module", "", "lib/util.py"),
            node(&mut text, b's', "fn", "helper", "lib/util.py"),
            node(&mut text, b's', "fn", "rare", "lib/util.py"),
            node(&mut text, b'm', "module", "", "app/a.py"),
            node(&mut text, b's', "fn", "one", "app/a.py"),
            node(&mut text, b's', "fn", "two", "app/a.py"),
        ];
        let (calls, imports, exact) = (text.push("calls"), text.push("imports"), text.push("exact"));
        let edge = |src, dst, edge| IndexedEdge { src, dst, edge, confidence: exact };
        // one and two call helper and one calls rare; app/a.py imports both, and its calls already cover those imports. Self-calls never count.
        let edges = vec![edge(4, 1, calls), edge(5, 1, calls), edge(4, 2, calls), edge(3, 1, imports), edge(3, 2, imports), edge(1, 1, calls)];
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
            parser: "dctx-structural/2".to_string(),
            ids: crate::index::IdIndex::default(),
        }
    }

    #[test]
    fn counts_calls_once_even_when_the_importing_file_calls_and_orders_by_degree() {
        let (hubs, dirs) = collect_hubs(&fixture());
        let seen: Vec<(&str, u32)> = hubs.iter().map(|hub| (hub.id.as_str(), hub.in_degree)).collect();
        assert_eq!(seen, [("s:lib/util.py#helper", 2), ("s:lib/util.py#rare", 1)]);
        let counts: Vec<(&str, u32, u32)> = dirs.iter().map(|dir| (dir.dir.as_str(), dir.files, dir.symbols)).collect();
        assert_eq!(counts, [("app/", 1, 2), ("lib/", 1, 2)]);
    }

    #[test]
    fn pages_carry_the_header_the_degree_and_the_directories() {
        let generation = fixture();
        let (hubs, dirs) = collect_hubs(&generation);
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
        let text = encode(&head, &hubs, &dirs, false);
        assert!(text.starts_with("# fixture hubs\ns:lib/util.py#helper\tfn\tlib/util.py\t2\n"), "{text}");
        assert!(text.ends_with("lib/\t1 files\t2 symbols\n"), "{text}");
        let json = encode(&head, &hubs, &dirs, true);
        assert!(json.contains("\"in_degree\":2") && json.contains("\"dirs\":[{\"dir\":\"app/\",\"files\":1,\"symbols\":2}"), "{json}");
        assert!(!json.contains("\"snapshot\""), "{json}");
        let meta = PageHead { meta: true, ..head };
        assert!(encode(&meta, &hubs, &dirs, false).starts_with("# fixture hubs v1:sha256:ab:1 g=1\n"));
    }
}
