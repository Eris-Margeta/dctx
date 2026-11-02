//! Sorted id index over a generation's nodes: one lookup is a binary search, not a scan of every node.
//! Implements `Generation::node_index`, the one place a stable id (`s:path#name~2`) becomes a node position.
//! Depends on [`super::id_matches`], which stays the single judge of a match; the index only narrows the candidates.
//! Built on the first lookup (verbs that never look up an id, such as `find`, pay nothing) and then kept with the generation.

use std::sync::OnceLock;

use super::{Generation, IndexedNode, TextBlob, id_matches};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn feed(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

/// Hash of the bytes of the stable id `node` writes, fed piece by piece in the order of [`super::write_stable_id`].
fn node_key(text: &TextBlob, node: &IndexedNode) -> u64 {
    let mut hash = feed(FNV_OFFSET, &[node.letter, b':']);
    hash = feed(hash, text.get(node.path).as_bytes());
    if !node.name.is_empty() {
        hash = feed(hash, b"#");
        hash = feed(hash, node.name.as_bytes());
    }
    if node.ordinal > 0 {
        hash = feed(hash, b"~");
        hash = feed(hash, node.ordinal.to_string().as_bytes());
    }
    hash
}

fn target_key(target: &str) -> u64 {
    feed(FNV_OFFSET, target.as_bytes())
}

/// `(id hash, node position)` sorted ascending, built for exactly `len` nodes.
#[derive(Debug)]
struct Built {
    len: usize,
    keys: Vec<(u64, u32)>,
}

/// Lazily built lookup table owned by a [`Generation`]. It is derived data: clones start empty and equality ignores it.
#[derive(Default)]
pub(crate) struct IdIndex {
    cell: OnceLock<Built>,
}

impl Clone for IdIndex {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for IdIndex {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for IdIndex {}

impl std::fmt::Debug for IdIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IdIndex")
    }
}

fn build(nodes: &[IndexedNode], text: &TextBlob) -> Built {
    let mut keys: Vec<(u64, u32)> = nodes
        .iter()
        .enumerate()
        .filter_map(|(position, node)| Some((node_key(text, node), u32::try_from(position).ok()?)))
        .collect();
    // The position is part of the key, so equal hashes stay in node order and the first match wins as in a scan.
    keys.sort_unstable();
    Built {
        len: nodes.len(),
        keys,
    }
}

/// The scan the index replaces. Also the answer when nodes changed after the index was built.
pub(crate) fn scan(nodes: &[IndexedNode], text: &TextBlob, target: &str) -> Option<usize> {
    nodes.iter().position(|node| id_matches(text, node, target))
}

impl Generation {
    /// Position of the first node whose stable id is `target`, or `None`. Same answer as [`scan`] on every input.
    pub(crate) fn node_index(&self, target: &str) -> Option<usize> {
        let built = self.ids.cell.get_or_init(|| build(&self.nodes, &self.text));
        if built.len != self.nodes.len() {
            return scan(&self.nodes, &self.text, target);
        }
        let key = target_key(target);
        let start = built.keys.partition_point(|entry| entry.0 < key);
        built.keys[start..]
            .iter()
            .take_while(|entry| entry.0 == key)
            .map(|entry| usize::try_from(entry.1).unwrap_or(usize::MAX))
            .find(|position| {
                self.nodes
                    .get(*position)
                    .is_some_and(|node| id_matches(&self.text, node, target))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FreshnessMode;
    use crate::index::write_node_id;

    fn node(letter: u8, ordinal: u32, name: &str, path: super::super::TextSpan) -> IndexedNode {
        IndexedNode {
            letter,
            ordinal,
            kind: "fn".to_string(),
            name: name.to_string(),
            path,
            lines: [1, 1],
            span: [0, 0],
            summary: String::new(),
            summary_source: String::new(),
            signature: String::new(),
        }
    }

    fn fixture() -> Generation {
        let mut text = TextBlob::default();
        let a = text.push("src/a.rs");
        let b = text.push("src/b.rs");
        let none = text.push("");
        let mut nodes = vec![
            node(b'm', 0, "", a),
            node(b'm', 0, "", b),
            node(b'm', 3, "", a), // empty name with an ordinal: never matches any target
            node(b's', 0, "run", a),
            node(b's', 2, "run", a),
            node(b's', 10, "run", a),
            node(b's', 0, "run~2", a), // writes the same id as the ordinal-2 node above: the first node wins
            node(b's', 0, "run", b),
            node(b't', 0, "Type.method", b),
            node(b'd', 0, "Heading #1", b),
            node(b's', 0, "x", none),
        ];
        for n in 0..2000u32 {
            nodes.push(node(
                b's',
                n % 7,
                &format!("f{n}"),
                if n % 2 == 0 { a } else { b },
            ));
        }
        Generation {
            id: 1,
            digest: String::new(),
            encoded_len: 0,
            files: Vec::new(),
            nodes,
            edges: Vec::new(),
            text,
            freshness: FreshnessMode::ImmutableSnapshot,
            taken_at: 0,
            epoch: 0,
            coverage: "structural".to_string(),
            completion: "complete".to_string(),
            parser: String::new(),
            ids: IdIndex::default(),
        }
    }

    /// The binary-search answer equals the linear scan for every written id, near-miss and junk target.
    #[test]
    fn index_answers_equal_the_linear_scan() {
        let generation = fixture();
        let mut targets = vec![
            String::new(),
            "s".to_string(),
            "s:".to_string(),
            "s:src/a.rs#run~02".to_string(),
            "s:src/a.rs#run~".to_string(),
            "s:src/a.rs#run~4294967296".to_string(),
            "s:src/a.rs#run~3".to_string(),
            "m:src/a.rs~3".to_string(),
            "m:src/a.rs#".to_string(),
            "x:src/a.rs".to_string(),
            "s:src/a.rs#nope".to_string(),
            "src/a.rs".to_string(),
        ];
        for node in &generation.nodes {
            let mut id = String::new();
            write_node_id(&mut id, &generation.text, node);
            targets.push(format!("{id}x"));
            targets.push(id[..id.len() - 1].to_string());
            targets.push(id);
        }
        let mut hits = 0;
        for target in &targets {
            let old = scan(&generation.nodes, &generation.text, target);
            assert_eq!(generation.node_index(target), old, "{target}");
            hits += usize::from(old.is_some());
        }
        assert!(hits > 2000, "the fixture must exercise real hits");
        // The first of two nodes that write the same id wins, as in a scan.
        assert_eq!(generation.node_index("s:src/a.rs#run~2"), Some(4));
        assert_eq!(generation.node_index("m:src/a.rs~3"), None);
    }

    /// Nodes pushed after the index was built are still found.
    #[test]
    fn a_grown_node_list_falls_back_to_the_scan() {
        let mut generation = fixture();
        assert_eq!(generation.node_index("s:late.rs#g"), None);
        let path = generation.text.push("late.rs");
        generation.nodes.push(node(b's', 0, "g", path));
        assert_eq!(
            generation.node_index("s:late.rs#g"),
            Some(generation.nodes.len() - 1)
        );
    }

    #[test]
    fn a_cloned_generation_equals_and_rebuilds() {
        let generation = fixture();
        assert_eq!(generation.node_index("s:src/a.rs#run"), Some(3));
        let copy = generation.clone();
        assert_eq!(copy, generation);
        assert_eq!(copy.node_index("s:src/b.rs#run"), Some(7));
    }
}
