// SPDX-License-Identifier: Apache-2.0
//! Export: the whole graph of a generation as one deterministic document (the `dctx map export` verb).
//! Two formats: a graph JSON (`nodes` and `edges`) and a Mermaid `flowchart LR` of the calls and imports edges.
//! Nodes are sorted by stable id and edges by (src, edge, dst, confidence), so equal generations give equal bytes.
//! The document is not an [`crate::domain::Answer`]: it has no page header, no cursor and no token cap.

use super::{Generation, write_node_id};

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
            other => Err(format!(
                "unknown export format: {other}; expected json or mermaid"
            )),
        }
    }
}

/// The whole graph of `generation` in `format`, ending in a newline.
#[cfg(test)]
pub(crate) fn export_graph(generation: &Generation, format: ExportFormat) -> String {
    let mut output = Vec::new();
    write_graph(generation, format, &mut output).expect("Vec writer");
    String::from_utf8(output).expect("UTF-8 graph")
}

/// Writes sorted graph records without retaining an encoded document. The ordering
/// tables contain ids and row references only; paths and edge attributes stay borrowed.
pub(crate) fn write_graph(
    generation: &Generation,
    format: ExportFormat,
    output: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let ids: Vec<String> = generation
        .nodes
        .iter()
        .map(|node| {
            let mut id = String::new();
            write_node_id(&mut id, &generation.text, node);
            id
        })
        .collect();
    let mut nodes: Vec<usize> = (0..ids.len()).collect();
    nodes.sort_by(|a, b| ids[*a].cmp(&ids[*b]));
    let edge_key = |edge: &super::IndexedEdge| {
        let src = ids.get(edge.src as usize).map(String::as_str);
        let dst = ids.get(edge.dst as usize).map(String::as_str);
        (
            src,
            generation.text.get(edge.edge),
            dst,
            generation.text.get(edge.confidence),
        )
    };
    let mut edges: Vec<_> = generation
        .edges
        .iter()
        .filter(|edge| edge_key(edge).0.is_some() && edge_key(edge).2.is_some())
        .collect();
    edges.sort_by_key(|edge| edge_key(edge));
    edges.dedup_by_key(|edge| edge_key(edge));
    match format {
        ExportFormat::Json => {
            output.write_all(b"{\"nodes\":[")?;
            for (ordinal, index) in nodes.iter().enumerate() {
                let node = &generation.nodes[*index];
                output.write_all(if ordinal == 0 {
                    b"\n{\"id\":"
                } else {
                    b",\n{\"id\":"
                })?;
                write_quote(output, &ids[*index])?;
                output.write_all(b",\"kind\":")?;
                write_quote(output, &node.kind)?;
                output.write_all(b",\"path\":")?;
                write_quote(output, generation.text.get(node.path))?;
                write!(output, ",\"lines\":[{},{}]}}", node.lines[0], node.lines[1])?;
            }
            output.write_all(b"],\"edges\":[")?;
            for (ordinal, edge) in edges.iter().enumerate() {
                output.write_all(if ordinal == 0 {
                    b"\n{\"src\":"
                } else {
                    b",\n{\"src\":"
                })?;
                write_quote(output, &ids[edge.src as usize])?;
                output.write_all(b",\"edge\":")?;
                write_quote(output, generation.text.get(edge.edge))?;
                output.write_all(b",\"dst\":")?;
                write_quote(output, &ids[edge.dst as usize])?;
                output.write_all(b",\"confidence\":")?;
                write_quote(output, generation.text.get(edge.confidence))?;
                output.write_all(b"}")?;
            }
            output.write_all(b"]}\n")
        }
        ExportFormat::Mermaid => {
            output.write_all(b"flowchart LR\n")?;
            for (ordinal, index) in nodes.iter().enumerate() {
                writeln!(
                    output,
                    "  n{ordinal}[\"{}\"]",
                    ids[*index].replace('"', "#quot;")
                )?;
            }
            let index_of = |id: &str| {
                nodes
                    .binary_search_by(|index| ids[*index].as_str().cmp(id))
                    .ok()
            };
            for edge in edges {
                let kind = generation.text.get(edge.edge);
                if (kind == "calls" || kind == "imports")
                    && let (Some(a), Some(b)) = (
                        index_of(&ids[edge.src as usize]),
                        index_of(&ids[edge.dst as usize]),
                    )
                {
                    writeln!(output, "  n{a} -->|{kind}| n{b}")?;
                }
            }
            Ok(())
        }
    }
}

fn write_quote(output: &mut dyn std::io::Write, text: &str) -> std::io::Result<()> {
    output.write_all(b"\"")?;
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if ch == '"' || ch == '\\' || ch < '\u{20}' {
            output.write_all(&text.as_bytes()[start..index])?;
            match ch {
                '"' => output.write_all(b"\\\"")?,
                '\\' => output.write_all(b"\\\\")?,
                '\n' => output.write_all(b"\\n")?,
                '\r' => output.write_all(b"\\r")?,
                '\t' => output.write_all(b"\\t")?,
                control => write!(output, "\\u{:04x}", u32::from(control))?,
            }
            start = index + ch.len_utf8();
        }
    }
    output.write_all(&text.as_bytes()[start..])?;
    output.write_all(b"\"")
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
        let nodes = vec![
            node(&mut text, b'm', "module", ""),
            node(&mut text, b's', "fn", "beta"),
            node(&mut text, b's', "fn", "alpha"),
        ];
        let (calls, contains, exact) = (
            text.push("calls"),
            text.push("contains"),
            text.push("exact"),
        );
        let edges = vec![
            IndexedEdge {
                src: 0,
                dst: 2,
                edge: contains,
                confidence: exact,
            },
            IndexedEdge {
                src: 2,
                dst: 1,
                edge: calls,
                confidence: exact,
            },
            IndexedEdge {
                src: 0,
                dst: 1,
                edge: contains,
                confidence: exact,
            },
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
            parser: "dctx-structural/3".to_string(),
            ids: crate::index::IdIndex::default(),
        }
    }

    #[test]
    fn writer_json_string_bytes_match_the_existing_encoder() {
        let text = "\"\\\n\r\t\0\u{8}\u{c} α\u{2028}\u{2029}";
        let mut expected = String::new();
        super::super::hubs::quote(&mut expected, text);
        let mut bytes = Vec::new();
        write_quote(&mut bytes, text).expect("quote");
        assert_eq!(bytes, expected.as_bytes());
    }

    #[test]
    fn json_and_mermaid_are_sorted_and_repeatable() {
        let generation = fixture();
        let json = export_graph(&generation, ExportFormat::Json);
        let ids: Vec<usize> = ["m:a.py", "s:a.py#alpha", "s:a.py#beta"]
            .iter()
            .map(|id| json.find(id).expect("id"))
            .collect();
        assert!(ids[0] < ids[1] && ids[1] < ids[2], "{json}");
        assert!(json.contains("{\"src\":\"s:a.py#alpha\",\"edge\":\"calls\",\"dst\":\"s:a.py#beta\",\"confidence\":\"exact\"}"), "{json}");
        assert!(json.contains("\"lines\":[1,2]"), "{json}");
        let mermaid = export_graph(&generation, ExportFormat::Mermaid);
        assert_eq!(
            mermaid,
            "flowchart LR\n  n0[\"m:a.py\"]\n  n1[\"s:a.py#alpha\"]\n  n2[\"s:a.py#beta\"]\n  n1 -->|calls| n2\n"
        );
        assert_eq!(mermaid, export_graph(&generation, ExportFormat::Mermaid));
        assert!(
            ExportFormat::parse("nope").is_err()
                && ExportFormat::parse("json") == Ok(ExportFormat::Json)
        );
    }
}
