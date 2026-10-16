//! Rust item names, signatures, attributes, and doc summaries.
//! Implements SPEC-MAP-MODE sections 5.1, 5.2 and 5.4.
//! Uses Rust syntax nodes; the traversal supplies lexical context.
use super::facts::{Facts, compact, lines, sentence, text};
use crate::{ParseError, body_fingerprint};
use dctx_map_model::{Node, NodeKind, StableId, Summary, SummarySource, SymbolKind};
use tree_sitter::Node as Syntax;

#[derive(Clone, Default)]
pub(super) struct Scope {
    pub owner: usize,
    pub name: String,
    pub test: bool,
    pub methods: bool,
}

pub(super) fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.into()
    } else {
        format!("{scope}::{name}")
    }
}

pub(super) fn item(
    node: Syntax<'_>,
    source: &str,
    scope: &Scope,
    facts: &mut Facts,
) -> Result<Option<Scope>, ParseError> {
    let symbol_kind = match node.kind() {
        "function_item" | "function_signature_item" => Some(if scope.methods {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        }),
        "struct_item" | "union_item" => Some(SymbolKind::Struct),
        "enum_item" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        "impl_item" => Some(SymbolKind::Impl),
        "const_item" | "static_item" => Some(SymbolKind::Const),
        "type_item" | "associated_type" => Some(SymbolKind::Type),
        "macro_definition" => Some(SymbolKind::Macro),
        "mod_item" if node.child_by_field_name("body").is_some() => None,
        _ => return Ok(None),
    };
    let is_test = node.kind() == "function_item" && test_attribute(node, source);
    let kind = if is_test {
        NodeKind::Test
    } else {
        NodeKind::Symbol
    };
    let local_name = if node.kind() == "impl_item" {
        let ty = node
            .child_by_field_name("type")
            .map(|n| compact(text(n, source)))
            .unwrap_or_default();
        let tr = node
            .child_by_field_name("trait")
            .map(|n| compact(text(n, source)));
        match tr {
            Some(tr) => format!("{ty}::impl<{tr}>"),
            None => format!("{ty}::impl"),
        }
    } else {
        node.child_by_field_name("name")
            .map(|n| text(n, source).trim_start_matches("r#").to_owned())
            .unwrap_or_default()
    };
    let name = qualify(&scope.name, &local_name);
    let body = node
        .child_by_field_name("body")
        .or_else(|| node.child_by_field_name("value"));
    let signature_end = body.map_or(node.end_byte(), |body| body.start_byte());
    let signature = source[node.start_byte()..signature_end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let docs = doc_comment(node, source);
    let summary = if docs.is_empty() {
        Summary {
            text: signature.clone(),
            source: SummarySource::Signature,
        }
    } else {
        Summary {
            text: sentence(&docs),
            source: SummarySource::Doc,
        }
    };
    let public = named_children(node)
        .iter()
        .any(|n| n.kind() == "visibility_modifier")
        || (scope.methods
            && facts.file.nodes[scope.owner].symbol_kind == Some(SymbolKind::Trait)
            && facts.file.nodes[scope.owner].public);
    let owner = facts.node(
        Node {
            id: StableId::new(kind, &facts.file.path, Some(&name), None)?,
            kind,
            symbol_kind,
            path: facts.file.path.clone(),
            lines: lines(node),
            name: name.clone(),
            signature,
            fingerprint: fingerprint(node, body, source),
            public,
            summary,
        },
        scope.owner,
    );
    let child_name = if node.kind() == "impl_item" {
        let ty = node
            .child_by_field_name("type")
            .map(|n| type_name(n, source))
            .unwrap_or_default();
        qualify(&scope.name, &ty)
    } else {
        name
    };
    Ok(Some(Scope {
        owner,
        name: child_name,
        test: is_test,
        methods: matches!(node.kind(), "impl_item" | "trait_item"),
    }))
}

pub(super) fn named_children(node: Syntax<'_>) -> Vec<Syntax<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn fingerprint(node: Syntax<'_>, body: Option<Syntax<'_>>, source: &str) -> String {
    if let Some(body) = body {
        return body_fingerprint(text(body, source));
    }
    if let Some(name) = node.child_by_field_name("name") {
        return body_fingerprint(&format!(
            "{} {}",
            &source[node.start_byte()..name.start_byte()],
            &source[name.end_byte()..node.end_byte()]
        ));
    }
    body_fingerprint(text(node, source))
}

fn type_name(node: Syntax<'_>, source: &str) -> String {
    let node = node
        .child_by_field_name("type")
        .filter(|_| node.kind() == "generic_type")
        .unwrap_or(node);
    compact(text(node, source))
}

fn test_attribute(node: Syntax<'_>, source: &str) -> bool {
    let mut previous = node.prev_named_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "attribute_item" => {
                let attr = compact(text(sibling, source));
                let path = attr
                    .trim_start_matches("#[")
                    .split(['(', ']'])
                    .next()
                    .unwrap_or_default();
                if path == "test" || path.ends_with("::test") {
                    return true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => break,
        }
        previous = sibling.prev_named_sibling();
    }
    false
}

fn doc_comment(node: Syntax<'_>, source: &str) -> String {
    let mut docs = Vec::new();
    let mut previous = node.prev_named_sibling();
    while let Some(sibling) = previous {
        let raw = text(sibling, source);
        match sibling.kind() {
            "line_comment" if raw.starts_with("///") && !raw.starts_with("////") => {
                docs.push(raw.trim_start_matches("///").trim().to_owned());
            }
            "block_comment" if raw.starts_with("/**") && !raw.starts_with("/***") => {
                docs.push(
                    raw.trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .lines()
                        .map(|line| line.trim().trim_start_matches('*').trim())
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            "attribute_item" => {}
            _ => break,
        }
        previous = sibling.prev_named_sibling();
    }
    docs.reverse();
    docs.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, TreeSitterParser};

    #[test]
    fn docs_attributes_and_methods_keep_their_meaning() {
        let source = b"pub struct A; impl A { /// Does work. More detail.\n pub fn work(&self) {} }\n#[tokio::test]\nasync fn checks() {}";
        let parsed = TreeSitterParser.parse("a.rs", source).unwrap();
        let method = parsed.nodes.iter().find(|n| n.name == "A::work").unwrap();
        assert_eq!(method.symbol_kind, Some(SymbolKind::Method));
        assert_eq!(method.summary.text, "Does work.");
        assert!(method.public);
        assert_eq!(parsed.nodes.last().unwrap().kind, NodeKind::Test);
        assert_eq!(qualify("", "item"), "item");
    }
}
