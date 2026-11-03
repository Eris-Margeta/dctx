// SPDX-License-Identifier: Apache-2.0
//! Unresolved Rust imports, calls, implementations, tests, and type uses.
//! Implements SPEC-MAP-MODE section 5.3 without name resolution.
//! Uses syntax fields and lexical scope; outputs resolver-ready raw sites.
use super::facts::{Facts, compact, lines, text};
use super::rust_items::{Scope, named_children};
use dctx_map_model::EdgeKind;
use tree_sitter::Node;

pub(super) fn references(node: Node<'_>, source: &str, scope: &Scope, facts: &mut Facts) {
    match node.kind() {
        "use_declaration" => imports(node, source, scope, facts),
        "mod_item" if node.child_by_field_name("body").is_none() => {
            if let Some(name) = node.child_by_field_name("name") {
                facts.reference(
                    0,
                    EdgeKind::Imports,
                    text(name, source).into(),
                    &scope.name,
                    None,
                    lines(node),
                );
            }
        }
        "impl_item" => {
            if let Some(trait_node) = node.child_by_field_name("trait") {
                facts.reference(
                    scope.owner,
                    EdgeKind::Implements,
                    compact(text(trait_node, source)),
                    &scope.name,
                    None,
                    lines(trait_node),
                );
            }
        }
        "call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                let function = if function.kind() == "generic_function" {
                    function.child_by_field_name("function").unwrap_or(function)
                } else {
                    function
                };
                if matches!(
                    function.kind(),
                    "identifier" | "scoped_identifier" | "field_expression"
                ) {
                    let name = compact(text(function, source));
                    facts.reference(
                        scope.owner,
                        EdgeKind::Calls,
                        name.clone(),
                        &scope.name,
                        None,
                        lines(node),
                    );
                    if scope.test {
                        facts.reference(
                            scope.owner,
                            EdgeKind::Tests,
                            name,
                            &scope.name,
                            None,
                            lines(node),
                        );
                    }
                }
            }
        }
        "type_identifier" | "scoped_type_identifier" => type_reference(node, source, scope, facts),
        _ => {}
    }
}

fn imports(node: Node<'_>, source: &str, scope: &Scope, facts: &mut Facts) {
    let Some(argument) = node.child_by_field_name("argument") else {
        return;
    };
    let public = named_children(node)
        .iter()
        .any(|n| n.kind() == "visibility_modifier");
    let mut pending = vec![(argument, String::new())];
    while let Some((part, prefix)) = pending.pop() {
        match part.kind() {
            "use_list" => {
                pending.extend(
                    named_children(part)
                        .into_iter()
                        .rev()
                        .filter(|n| !matches!(n.kind(), "line_comment" | "block_comment"))
                        .map(|n| (n, prefix.clone())),
                );
            }
            "scoped_use_list" => {
                let path = part
                    .child_by_field_name("path")
                    .map(|n| compact(text(n, source)))
                    .unwrap_or_default();
                if let Some(list) = part.child_by_field_name("list").or_else(|| {
                    named_children(part)
                        .into_iter()
                        .find(|n| n.kind() == "use_list")
                }) {
                    pending.push((list, join(&prefix, &path)));
                }
            }
            _ => {
                let (name, alias) = if part.kind() == "use_as_clause" {
                    let path = part
                        .child_by_field_name("path")
                        .map(|n| compact(text(n, source)))
                        .unwrap_or_default();
                    let alias = part
                        .child_by_field_name("alias")
                        .map(|n| text(n, source).into());
                    (join(&prefix, &path), alias)
                } else {
                    (join(&prefix, &compact(text(part, source))), None)
                };
                facts.reference(
                    0,
                    EdgeKind::Imports,
                    name.clone(),
                    &scope.name,
                    alias.clone(),
                    lines(part),
                );
                if public {
                    facts.reference(
                        0,
                        EdgeKind::Reexports,
                        name,
                        &scope.name,
                        alias,
                        lines(part),
                    );
                }
            }
        }
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.into()
    } else if name == "self" {
        prefix.into()
    } else {
        format!("{prefix}::{name}")
    }
}

fn type_reference(node: Node<'_>, source: &str, scope: &Scope, facts: &mut Facts) {
    if let Some(parent) = node.parent()
        && (parent.kind() == "scoped_type_identifier"
            || parent.child_by_field_name("name") == Some(node)
            || (parent.kind() == "impl_item" && parent.child_by_field_name("trait") == Some(node)))
    {
        return;
    }
    let name = compact(text(node, source));
    if name != "Self" {
        facts.reference(
            scope.owner,
            EdgeKind::UsesType,
            name,
            &scope.name,
            None,
            lines(node),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, TreeSitterParser};

    #[test]
    fn nested_imports_aliases_self_and_globs_are_flattened() {
        let facts = TreeSitterParser
            .parse("lib.rs", b"use crate::a::{self, b::{C as D, E}, *};")
            .unwrap();
        let imports = facts
            .references
            .iter()
            .map(|r| (r.name.as_str(), r.alias.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            imports,
            [
                ("crate::a", None),
                ("crate::a::b::C", Some("D")),
                ("crate::a::b::E", None),
                ("crate::a::*", None)
            ]
        );
        assert_eq!(join("", "::absolute::Item"), "::absolute::Item");
    }

    #[test]
    fn method_and_generic_calls_keep_syntactic_targets() {
        let facts = TreeSitterParser
            .parse(
                "lib.rs",
                b"fn run() { crate::work::<Thing>(); worker.run(); }",
            )
            .unwrap();
        let calls = facts
            .references
            .iter()
            .filter(|r| r.kind == EdgeKind::Calls)
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(calls, ["crate::work", "worker.run"]);
    }
}
