//! JavaScript and TypeScript traversal behind the Parser port.
//! Implements SPEC-MAP-MODE sections 5 and 6.2 with pinned grammars.
//! Item and reference extractors share lexical ownership without runtime state.
use super::facts::{Facts, text};
use super::javascript_items::{Scope, children, item, jsdoc};
use super::javascript_references::references;
use crate::{Language, ParseError};

pub(super) fn parse(source: &str, facts: &mut Facts) -> Result<(), ParseError> {
    let language = match facts.file.language {
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    };
    let tree = super::tree(source, language)?;
    let root = tree.root_node();
    if root.has_error() {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if node.is_error() || node.is_missing() {
                return Err(ParseError::Syntax {
                    path: facts.file.path.clone(),
                    line: node.start_position().row as u32 + 1,
                });
            }
            pending.extend(children(node).into_iter().rev().filter(|n| n.has_error()));
        }
        return Err(ParseError::Syntax {
            path: facts.file.path.clone(),
            line: 1,
        });
    }
    if let Some(first) = children(root).first()
        && first.kind() == "comment"
        && let Some(summary) = jsdoc(text(*first, source))
    {
        facts.file.nodes[0].summary.text = summary;
    }
    let mut pending = vec![(root, Scope::default())];
    while let Some((node, scope)) = pending.pop() {
        let context = item(node, source, &scope, facts)?.unwrap_or(scope);
        references(node, source, &context, facts);
        if matches!(node.kind(), "import_statement" | "comment") {
            continue;
        }
        pending.extend(
            children(node)
                .into_iter()
                .rev()
                .map(|n| (n, context.clone())),
        );
    }
    Ok(())
}
