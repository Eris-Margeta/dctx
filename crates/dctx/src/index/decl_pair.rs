// SPDX-License-Identifier: Apache-2.0
//! Declaration-to-definition pairing for the neighbors verb.
//! A declaration-only symbol (a C/C++ header prototype, an `.mli` `val`, or a `.d.ts` `declare`) has no call edges of its own,
//! so a neighbors probe on it scores nothing. When the declaration has a unique definition in the same project, the probe
//! answers with that definition's records, and the answer's first record names the definition id once.
//! The pairing is deterministic and byte-stable: a definition's own answer is never redirected.

use crate::domain::AnswerRecord;

use super::{Generation, IndexedNode, write_node_id};

/// The definition extensions of `path`'s language family, or `None` when `path` is not a declaration-only file.
///
/// A header's family is C/C++; an `.mli` family is OCaml; a `.d.ts` family is TypeScript/JavaScript.
fn definition_extensions(path: &str) -> Option<&'static [&'static str]> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mli") {
        return Some(&["ml"][..]);
    }
    if lower.ends_with(".hpp")
        || lower.ends_with(".hxx")
        || lower.ends_with(".hh")
        || lower.ends_with(".h")
    {
        return Some(&["c", "cc", "cpp", "cxx", "m"][..]);
    }
    if lower.ends_with(".d.ts") || lower.ends_with(".d.mts") || lower.ends_with(".d.cts") {
        return Some(&["ts", "js", "mts", "mjs", "cts", "cjs"][..]);
    }
    None
}

/// The filename without its declaration-only extension, or `None` when no declaration extension applies.
fn declaration_stem(file: &str) -> Option<&str> {
    [
        ".mli", ".d.ts", ".d.mts", ".d.cts", ".hpp", ".hxx", ".hh", ".h",
    ]
    .into_iter()
    .find_map(|suffix| file.strip_suffix(suffix))
}

/// True when `path` ends with one of `extensions`, compared case-insensitively.
fn has_definition_extension(path: &str, extensions: &[&str]) -> bool {
    let lower = path.to_ascii_lowercase();
    extensions
        .iter()
        .any(|extension| lower.ends_with(&format!(".{extension}")))
}

/// True when `node` is a declaration-only symbol: a `fn` in a header, `.mli` or `.d.ts` file.
pub(crate) fn is_declaration_only(generation: &Generation, node: &IndexedNode) -> bool {
    is_declaration_only_path(generation.text.get(node.path), node)
}

/// True when `path` holds a declaration-only symbol: a `fn` in a header, `.mli` or `.d.ts` file.
///
/// `refs.rs` excludes these from its callee index, so a call resolves to the paired definition (the real implementation).
pub(crate) fn is_declaration_only_path(path: &str, node: &IndexedNode) -> bool {
    node.kind == "fn" && !node.name.is_empty() && definition_extensions(path).is_some()
}

/// The generation index of `node_index`'s paired definition, or `None` when there is no unique pairing.
///
/// The sibling file with the same stem wins (util.h -> util.c, api.d.ts -> api.ts). Otherwise the one definition with that
/// qualified name in the project's same language family; several or none means no pairing (today's answer).
pub(crate) fn paired_definition(generation: &Generation, node_index: u32) -> Option<u32> {
    let text = &generation.text;
    let node = generation.nodes.get(usize::try_from(node_index).ok()?)?;
    if !is_declaration_only(generation, node) {
        return None;
    }
    let path = text.get(node.path);
    let extensions = definition_extensions(path)?;
    let (dir, file) = path
        .rsplit_once('/')
        .map_or(("", path), |(dir, file)| (dir, file));
    let stem = declaration_stem(file)?;
    let name = node.name.as_str();
    let mut sibling = Vec::new();
    for (index, other) in generation.nodes.iter().enumerate() {
        if other.kind != "fn" || other.name != name {
            continue;
        }
        let other_path = text.get(other.path);
        let (other_dir, other_file) = other_path
            .rsplit_once('/')
            .map_or(("", other_path), |(dir, file)| (dir, file));
        if other_dir == dir
            && extensions
                .iter()
                .any(|extension| other_file == format!("{stem}.{extension}"))
        {
            sibling.push(index);
        }
    }
    if sibling.len() == 1 {
        return u32::try_from(sibling[0]).ok();
    }
    let mut anywhere = Vec::new();
    for (index, other) in generation.nodes.iter().enumerate() {
        if other.kind != "fn" || other.name != name {
            continue;
        }
        let other_path = text.get(other.path);
        if other_path != path && has_definition_extension(other_path, extensions) {
            anywhere.push(index);
        }
    }
    if anywhere.len() == 1 {
        return u32::try_from(anywhere[0]).ok();
    }
    None
}

/// The first record that names the paired definition of `target`, or `None` when `target` is not a declaration-only symbol
/// or has no unique pairing.
///
/// The record is `pair:<definition id>` of kind `definition`, so a `--json` page greps the definition id once.
pub(crate) fn pair_record(generation: &Generation, target: &str) -> Option<AnswerRecord> {
    let text = &generation.text;
    let root = generation.node_index(target)?;
    let definition = paired_definition(generation, u32::try_from(root).ok()?)?;
    let node = generation.nodes.get(usize::try_from(definition).ok()?)?;
    let mut id = String::new();
    write_node_id(&mut id, text, node);
    let mut record = AnswerRecord::basic(format!("pair:{id}"), "definition", text.get(node.path));
    record.lines = node.lines;
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FreshnessMode;
    use crate::index::{IndexedEdge, IndexedNode, TextBlob};

    /// A generation with a header prototype `add` and its definition in the sibling `util.c`.
    fn fixture() -> (Generation, u32, u32) {
        let mut text = TextBlob::default();
        let header = text.push("c/util.h");
        let source = text.push("c/util.c");
        let other = text.push("lib/util.c");
        let calls = text.push("calls");
        let name = text.push("name");
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut push = |nodes: &mut Vec<IndexedNode>, path, node_name: &str, kind: &str| {
            nodes.push(IndexedNode {
                letter: b's',
                ordinal: 0,
                kind: kind.to_string(),
                name: node_name.to_string(),
                path,
                lines: [1, 1],
                span: [0, 0],
                summary: String::new(),
                summary_source: "sig".to_string(),
                signature: String::new(),
            });
            let index = u32::try_from(nodes.len() - 1).unwrap_or(0);
            edges.push(IndexedEdge {
                src: 0,
                dst: index,
                edge: calls,
                confidence: name,
            });
            index
        };
        let decl = push(&mut nodes, header, "add", "fn");
        let def = push(&mut nodes, source, "add", "fn");
        let _other_def = push(&mut nodes, other, "add", "fn");
        let generation = Generation {
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
            ids: crate::index::IdIndex::default(),
        };
        (generation, decl, def)
    }

    #[test]
    fn a_header_prototype_pairs_with_its_sibling_definition() {
        let (generation, decl, def) = fixture();
        assert_eq!(paired_definition(&generation, decl), Some(def));
    }

    #[test]
    fn an_ambiguous_paired_definition_is_not_reported() {
        let (generation, decl, def) = fixture();
        // A second definition with the same name in the sibling file makes the pairing ambiguous.
        let mut generation = generation;
        let mut node = generation.nodes[usize::try_from(def).unwrap_or(0)].clone();
        node.ordinal = 1;
        node.lines = [2, 2];
        generation.nodes.push(node);
        let index = u32::try_from(generation.nodes.len() - 1).unwrap_or(0);
        let calls = generation.edges[0].edge;
        let confidence = generation.edges[0].confidence;
        generation.edges.push(IndexedEdge {
            src: 0,
            dst: index,
            edge: calls,
            confidence,
        });
        assert_eq!(paired_definition(&generation, decl), None);
    }

    #[test]
    fn a_definition_is_not_itself_declaration_only() {
        let (generation, _, def) = fixture();
        assert!(!is_declaration_only(
            &generation,
            &generation.nodes[usize::try_from(def).unwrap_or(0)]
        ));
    }
}
