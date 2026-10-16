//! Iterative traversal of Rust declarations and reference sites.
//! Implements SPEC-MAP-MODE sections 5.1, 5.3 and 6.2.
//! Uses the pinned Rust grammar, item extraction, and reference extraction.
use super::facts::Facts;
use super::rust_items::{Scope, item, named_children};
use super::rust_references::references;
use crate::ParseError;

pub(super) fn parse(source: &str, facts: &mut Facts) -> Result<(), ParseError> {
    let tree = super::tree(source, tree_sitter_rust::LANGUAGE.into())?;
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
            pending.extend(
                named_children(node)
                    .into_iter()
                    .rev()
                    .filter(|n| n.has_error()),
            );
        }
        return Err(ParseError::Syntax {
            path: facts.file.path.clone(),
            line: 1,
        });
    }
    let mut pending = vec![(root, Scope::default())];
    while let Some((node, scope)) = pending.pop() {
        let context = item(node, source, &scope, facts)?.unwrap_or(scope);
        references(node, source, &context, facts);
        if matches!(
            node.kind(),
            "macro_definition" | "macro_invocation" | "use_declaration"
        ) {
            continue;
        }
        pending.extend(
            named_children(node)
                .into_iter()
                .rev()
                .map(|child| (child, context.clone())),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{Parser, TreeSitterParser};

    #[test]
    fn macro_bodies_are_not_expanded_into_false_symbols() {
        let source = b"macro_rules! make { () => { fn phantom() {} } } make!(); fn real() {}";
        let facts = TreeSitterParser.parse("lib.rs", source).unwrap();
        assert_eq!(
            facts
                .nodes
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["lib.rs", "make", "real"]
        );
    }
}
