// SPDX-License-Identifier: Apache-2.0
//! Export: the whole graph of a generation as one deterministic document (the `dctx map export` verb).
//! Two formats: a graph JSON (`nodes` and `edges`) and a Mermaid `flowchart LR` of the calls and imports edges.
//! Nodes are sorted by stable id and edges by (src, edge, dst, confidence), so equal generations give equal bytes.
//! The document is not an [`crate::domain::Answer`]: it has no page header, no cursor and no token cap.

use std::fmt::Write as _;

use super::hubs::quote;
use super::{write_node_id, Generation};

/// Output format of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    /// `{"nodes":[...],"edges":[...]}`.
    Json,
    /// `flowchart LR` with one line per calls or imports edge.
    Mermaid,
}

impl ExportFormat {
    /// Parses `json` or `mermaid`.
    /// # Errors
    /// The unknown name.
    pub(crate) fn parse(name: &str) -> Result<Self, String> {
        match name {
            "json" => Ok(Self::Json),
            "mermaid" => Ok(Self::Mermaid),
            other => Err(format!("unknown export format: {other}; expected json or mermaid")),
        }
    }
}

/// One node of the export.
struct ExportNode {
    /// Stable id.
    id: String,
    /// Wire kind.
    kind: String,
    /// Root-relative path of the file.
    path: String,
    /// Inclusive 1-based lines.
    lines: [u32; 2],
}

/// One edge of the export, by stable ids.
struct ExportEdge {
    /// Stable id of the source.
    src: String,
    /// Edge name (`contains`, `calls`, `imports`).
    edge: String,
    /// Stable id of the destination.
    dst: String,
    /// Confidence label.
    confidence: String,
}

/// Stable id of node `index`, or `None` when it is out of range.
fn id_of(generation: &Generation, index: u32) -> Option<String> {
    let node = generation.nodes.get(usize::try_from(index).ok()?)?;
    let mut id = String::new();
    write_node_id(&mut id, &generation.text, node);
    Some(id)
}

/// Every node sorted by id and every edge sorted by (src, edge, dst, confidence).
fn collect(generation: &Generation) -> (Vec<ExportNode>, Vec<ExportEdge>) {
    let text = &generation.text;
    let mut nodes: Vec<ExportNode> = generation
        .nodes
        .iter()
        .map(|node| {
            let mut id = String::new();
            write_node_id(&mut id, text, node);
            ExportNode { id, kind: node.kind.clone(), path: text.get(node.path).to_string(), lines: node.lines }
        })
        .collect();
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    let mut edges: Vec<ExportEdge> = generation
        .edges
        .iter()
        .filter_map(|edge| {
            let (src, dst) = (id_of(generation, edge.src)?, id_of(generation, edge.dst)?);
            Some(ExportEdge { src, edge: text.get(edge.edge).to_string(), dst, confidence: text.get(edge.confidence).to_string() })
        })
        .collect();
    edges.sort_by(|a, b| (&a.src, &a.edge, &a.dst, &a.confidence).cmp(&(&b.src, &b.edge, &b.dst, &b.confidence)));
    edges.dedup_by(|a, b| (&a.src, &a.edge, &a.dst, &a.confidence) == (&b.src, &b.edge, &b.dst, &b.confidence));
    (nodes, edges)
}

/// The whole graph of `generation` in `format`, ending in a newline.
pub(crate) fn export_graph(generation: &Generation, format: ExportFormat) -> String {
    let (nodes, edges) = collect(generation);
    match format {
        ExportFormat::Json => encode_json(&nodes, &edges),
        ExportFormat::Mermaid => encode_mermaid(&nodes, &edges),
    }
}

/// `{"nodes":[{"id","kind","path","lines"}...],"edges":[{"src","edge","dst","confidence"}...]}`.
fn encode_json(nodes: &[ExportNode], edges: &[ExportEdge]) -> String {
    let mut out = String::from("{\"nodes\":[");
    for (index, node) in nodes.iter().enumerate() {
        out.push_str(if index > 0 { ",\n{\"id\":" } else { "\n{\"id\":" });
        quote(&mut out, &node.id);
        out.push_str(",\"kind\":");
        quote(&mut out, &node.kind);
        out.push_str(",\"path\":");
        quote(&mut out, &node.path);
        let _ = write!(out, ",\"lines\":[{},{}]}}", node.lines[0], node.lines[1]);
    }
    out.push_str("],\"edges\":[");
    for (index, edge) in edges.iter().enumerate() {
        out.push_str(if index > 0 { ",\n{\"src\":" } else { "\n{\"src\":" });
        quote(&mut out, &edge.src);
        out.push_str(",\"edge\":");
        quote(&mut out, &edge.edge);
        out.push_str(",\"dst\":");
        quote(&mut out, &edge.dst);
        out.push_str(",\"confidence\":");
        quote(&mut out, &edge.confidence);
        out.push('}');
    }
    out.push_str("]}\n");
    out
}

/// `flowchart LR`, one `n<index>["<id>"]` line per node and one `n<a> -->|calls| n<b>` line per calls or imports edge.
fn encode_mermaid(nodes: &[ExportNode], edges: &[ExportEdge]) -> String {
    let mut out = String::from("flowchart LR\n");
    for (index, node) in nodes.iter().enumerate() {
        let _ = writeln!(out, "  n{index}[\"{}\"]", node.id.replace('"', "#quot;"));
    }
    let index_of = |id: &str| nodes.binary_search_by(|node| node.id.as_str().cmp(id)).ok();
    for edge in edges.iter().filter(|edge| edge.edge == "calls" || edge.edge == "imports") {
        if let (Some(a), Some(b)) = (index_of(&edge.src), index_of(&edge.dst)) {
            let _ = writeln!(out, "  n{a} -->|{}| n{b}", edge.edge);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FreshnessMode;
    use crate::index::{IndexedEdge, IndexedNode, TextBlob};

    fn node(text: &mut TextBlob, letter: u8, kind: &str, name: &str) -> IndexedNode {
        IndexedNode {
            letter,
            ordinal: 0,
            kind: kind.to_string(),
            name: name.to_string(),
            path: text.push("a.py"),
            lines: [1, 2],
            span: [0, 0],
            summary: String::new(),
            summary_source: String::new(),
            signature: String::new(),
        }
    }

    fn fixture() -> Generation {
        let mut text = TextBlob::default();
        // Stored out of id order on purpose: beta before alpha.
        let nodes = vec![node(&mut text, b'm', "module", ""), node(&mut text, b's', "fn", "beta"), node(&mut text, b's', "fn", "alpha")];
        let (calls, contains, exact) = (text.push("calls"), text.push("contains"), text.push("exact"));
        let edges = vec![
            IndexedEdge { src: 0, dst: 2, edge: contains, confidence: exact },
            IndexedEdge { src: 2, dst: 1, edge: calls, confidence: exact },
            IndexedEdge { src: 0, dst: 1, edge: contains, confidence: exact },
        ];
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
    fn json_and_mermaid_are_sorted_and_repeatable() {
        let generation = fixture();
        let json = export_graph(&generation, ExportFormat::Json);
        let ids: Vec<usize> = ["m:a.py", "s:a.py#alpha", "s:a.py#beta"].iter().map(|id| json.find(id).expect("id")).collect();
        assert!(ids[0] < ids[1] && ids[1] < ids[2], "{json}");
        assert!(json.contains("{\"src\":\"s:a.py#alpha\",\"edge\":\"calls\",\"dst\":\"s:a.py#beta\",\"confidence\":\"exact\"}"), "{json}");
        assert!(json.contains("\"lines\":[1,2]"), "{json}");
        let mermaid = export_graph(&generation, ExportFormat::Mermaid);
        assert_eq!(mermaid, "flowchart LR\n  n0[\"m:a.py\"]\n  n1[\"s:a.py#alpha\"]\n  n2[\"s:a.py#beta\"]\n  n1 -->|calls| n2\n");
        assert_eq!(mermaid, export_graph(&generation, ExportFormat::Mermaid));
        assert!(ExportFormat::parse("nope").is_err() && ExportFormat::parse("json") == Ok(ExportFormat::Json));
    }
}
