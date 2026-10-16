//! JavaScript/TypeScript declarations, test blocks and JSDoc summaries.
//! Implements SPEC-MAP-MODE sections 5.1, 5.2 and 5.4.
//! Consumed by the syntax walker using public model constructors.
use super::facts::{Facts, lines, sentence, text};
use crate::{ParseError, body_fingerprint};
use dctx_map_model::{Node, NodeKind, StableId, Summary, SummarySource, SymbolKind};
use tree_sitter::Node as Syntax;

#[derive(Clone, Default)]
pub(super) struct Scope {
    pub owner: usize,
    pub name: String,
    pub test: bool,
}

pub(super) fn children(node: Syntax<'_>) -> Vec<Syntax<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub(super) fn string(node: Syntax<'_>, source: &str) -> Option<String> {
    (node.kind() == "string").then(|| {
        let value = text(node, source);
        value[1..value.len() - 1].to_owned()
    })
}

pub(super) fn test_name(node: Syntax<'_>, source: &str) -> Option<String> {
    if node.kind() != "call_expression" {
        return None;
    }
    let function = node.child_by_field_name("function")?;
    let spelling = text(function, source);
    let mut parts = spelling.split('.');
    if !matches!(parts.next()?, "test" | "it" | "describe")
        || !parts.all(|part| matches!(part, "only" | "skip" | "todo" | "concurrent"))
    {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    string(*children(arguments).first()?, source)
}

pub(super) fn item(
    node: Syntax<'_>,
    source: &str,
    scope: &Scope,
    facts: &mut Facts,
) -> Result<Option<Scope>, ParseError> {
    let test = test_name(node, source);
    let symbol_kind = match node.kind() {
        "function_declaration" | "generator_function_declaration" | "function_signature" => {
            SymbolKind::Function
        }
        "class_declaration" | "abstract_class_declaration" => SymbolKind::Class,
        "method_definition" | "method_signature" | "abstract_method_signature" => {
            SymbolKind::Method
        }
        "interface_declaration" => SymbolKind::Interface,
        "type_alias_declaration" => SymbolKind::Type,
        "enum_declaration" => SymbolKind::Enum,
        "function_expression" | "arrow_function" | "generator_function"
            if node
                .parent()
                .is_some_and(|p| p.kind() == "export_statement") =>
        {
            SymbolKind::Function
        }
        "class"
            if node
                .parent()
                .is_some_and(|p| p.kind() == "export_statement") =>
        {
            SymbolKind::Class
        }
        "variable_declarator" | "field_definition" | "public_field_definition" | "pair" => {
            let Some(value) = node.child_by_field_name("value") else {
                return Ok(None);
            };
            if !matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "generator_function"
            ) {
                return Ok(None);
            }
            if matches!(node.kind(), "field_definition" | "public_field_definition") {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            }
        }
        "call_expression" if test.is_some() => SymbolKind::Function,
        _ => return Ok(None),
    };
    let local = test.clone().or_else(|| {
        node.child_by_field_name("name")
            .or_else(|| node.child_by_field_name("property"))
            .or_else(|| node.child_by_field_name("key"))
            .map(|n| string(n, source).unwrap_or_else(|| text(n, source).into()))
    });
    let local = local.or_else(|| exported(node).then(|| "default".into()));
    let Some(local) = local else { return Ok(None) };
    let name = if scope.name.is_empty() {
        local
    } else {
        format!("{}::{local}", scope.name)
    };
    let kind = if test.is_some() {
        NodeKind::Test
    } else {
        NodeKind::Symbol
    };
    let value = node.child_by_field_name("value").unwrap_or(node);
    let body = value.child_by_field_name("body");
    let signature = source[node.start_byte()..body.map_or(node.end_byte(), |n| n.start_byte())]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let summary = match doc(node, source) {
        Some(text) => Summary {
            text,
            source: SummarySource::Doc,
        },
        None => Summary {
            text: signature.clone(),
            source: SummarySource::Signature,
        },
    };
    let owner = facts.node(
        Node {
            id: StableId::new(kind, &facts.file.path, Some(&name), None)?,
            kind,
            symbol_kind: Some(symbol_kind),
            path: facts.file.path.clone(),
            lines: lines(node),
            name: name.clone(),
            signature,
            fingerprint: body_fingerprint(text(body.unwrap_or(value), source)),
            public: exported(node),
            summary,
        },
        scope.owner,
    );
    Ok(Some(Scope {
        owner,
        name,
        test: scope.test || test.is_some(),
    }))
}

fn exported(mut node: Syntax<'_>) -> bool {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "export_statement" => return true,
            "lexical_declaration" | "variable_declaration" => node = parent,
            _ => return false,
        }
    }
    false
}

fn doc(mut node: Syntax<'_>, source: &str) -> Option<String> {
    if let Some(parent) = node.parent()
        && matches!(
            parent.kind(),
            "lexical_declaration" | "variable_declaration"
        )
    {
        node = parent;
    }
    if let Some(parent) = node.parent()
        && parent.kind() == "export_statement"
    {
        node = parent;
    }
    let previous = node.prev_named_sibling()?;
    if previous.kind() != "comment"
        || !source[previous.end_byte()..node.start_byte()]
            .trim()
            .is_empty()
    {
        return None;
    }
    jsdoc(text(previous, source))
}

pub(super) fn jsdoc(comment: &str) -> Option<String> {
    let body = comment.strip_prefix("/**")?.strip_suffix("*/")?;
    body.lines()
        .map(|line| line.trim().trim_start_matches('*').trim())
        .find(|line| !line.is_empty() && !line.starts_with('@'))
        .map(sentence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, TreeSitterParser};

    #[test]
    fn jsdoc_requires_doc_syntax_and_uses_first_description_line() {
        assert_eq!(jsdoc("/* ordinary */"), None);
        assert_eq!(
            jsdoc("/**\n * First line. More.\n * Second line.\n */"),
            Some("First line.".into())
        );
        let facts = TreeSitterParser
            .parse(
                "x.js",
                b"/** Module docs. */\nexport default function() { run(); }",
            )
            .unwrap();
        assert_eq!(facts.nodes[0].summary.text, "Module docs.");
        assert_eq!(facts.nodes[1].name, "default");
        assert!(facts.nodes[1].public);
        assert_eq!(facts.nodes[1].summary.source, SummarySource::Doc);
    }

    #[test]
    fn named_expressions_and_class_field_arrows_own_their_calls() {
        let facts = TreeSitterParser
            .parse(
                "x.js",
                b"const f = function internal() { work(); }; class C { run = () => f(); }",
            )
            .unwrap();
        assert_eq!(
            facts
                .nodes
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["x.js", "f", "C", "C::run"]
        );
        assert_eq!(facts.nodes[3].symbol_kind, Some(SymbolKind::Method));
        assert_eq!(facts.references[0].src, facts.nodes[1].id);
        assert_eq!(facts.references[1].src, facts.nodes[3].id);
    }
}
