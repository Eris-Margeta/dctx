//! JavaScript/TypeScript imports, exports, calls and type references.
//! Implements SPEC-MAP-MODE sections 5.3 and 6.2 without resolution.
//! The walker supplies lexical source ownership; the resolver consumes spellings.
use super::facts::{Facts, compact, lines, text};
use super::javascript_items::{Scope, children, string, test_name};
use dctx_map_model::EdgeKind;
use tree_sitter::Node;

pub(super) fn references(node: Node<'_>, source: &str, scope: &Scope, facts: &mut Facts) {
    match node.kind() {
        "import_statement" | "export_statement" => imports(node, source, scope, facts),
        "call_expression" => {
            if require(node, source, scope, facts) || test_name(node, source).is_some() {
                return;
            }
            if let Some(function) = node.child_by_field_name("function")
                && matches!(function.kind(), "identifier" | "member_expression")
            {
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
        "type_identifier" | "nested_type_identifier" => {
            if let Some(parent) = node.parent()
                && ((parent.child_by_field_name("name") == Some(node)
                    && matches!(
                        parent.kind(),
                        "class_declaration"
                            | "abstract_class_declaration"
                            | "interface_declaration"
                            | "type_alias_declaration"
                            | "enum_declaration"
                            | "type_parameter"
                    ))
                    || matches!(
                        parent.kind(),
                        "nested_type_identifier" | "import_specifier" | "export_specifier"
                    ))
            {
                return;
            }
            facts.reference(
                scope.owner,
                EdgeKind::UsesType,
                compact(text(node, source)),
                &scope.name,
                None,
                lines(node),
            );
        }
        _ => {}
    }
}

fn imports(node: Node<'_>, source: &str, scope: &Scope, facts: &mut Facts) {
    let module = node
        .child_by_field_name("source")
        .and_then(|n| string(n, source));
    let export = node.kind() == "export_statement";
    let kind = if export {
        EdgeKind::Reexports
    } else {
        EdgeKind::Imports
    };
    let mut bindings = Vec::new();
    let mut pending = children(node).into_iter().rev().collect::<Vec<_>>();
    while let Some(part) = pending.pop() {
        match part.kind() {
            "import_specifier" | "export_specifier" => {
                if let Some(name) = part.child_by_field_name("name") {
                    let name = string(name, source).unwrap_or_else(|| text(name, source).into());
                    let alias = part
                        .child_by_field_name("alias")
                        .map(|n| text(n, source).into());
                    bindings.push((name, alias, lines(part)));
                }
            }
            "namespace_import" | "namespace_export" => {
                let alias = children(part).last().map(|n| text(*n, source).into());
                bindings.push(("*".into(), alias, lines(part)));
            }
            "identifier" if !export => {
                bindings.push((
                    "default".into(),
                    Some(text(part, source).into()),
                    lines(part),
                ));
            }
            "import_clause" | "named_imports" | "export_clause" => {
                pending.extend(children(part).into_iter().rev());
            }
            _ => {}
        }
    }
    if bindings.is_empty() {
        if let Some(module) = module {
            let target = if export {
                format!("{module}::*")
            } else {
                module
            };
            facts.reference(0, kind, target, &scope.name, None, lines(node));
        }
        return;
    }
    for (name, alias, site) in bindings {
        let target = module.as_ref().map_or_else(
            || name.clone(),
            |module| {
                if name == "*" && alias.is_some() {
                    module.clone()
                } else {
                    format!("{module}::{name}")
                }
            },
        );
        facts.reference(0, kind, target, &scope.name, alias, site);
    }
}

fn require(node: Node<'_>, source: &str, scope: &Scope, facts: &mut Facts) -> bool {
    let Some(function) = node.child_by_field_name("function") else {
        return false;
    };
    if function.kind() != "identifier" || text(function, source) != "require" {
        return false;
    }
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return false;
    };
    let arguments = children(arguments);
    let Some(module) = arguments.first().and_then(|n| string(*n, source)) else {
        return false;
    };
    let binding = node
        .parent()
        .filter(|p| p.kind() == "variable_declarator")
        .and_then(|p| p.child_by_field_name("name"));
    match binding {
        Some(binding) if binding.kind() == "object_pattern" => {
            for part in children(binding) {
                let (name, alias) = if part.kind() == "pair_pattern" {
                    let name = part
                        .child_by_field_name("key")
                        .map(|n| text(n, source))
                        .unwrap_or_default();
                    let alias = part
                        .child_by_field_name("value")
                        .map(|n| text(n, source).into());
                    (name, alias)
                } else if part.kind() == "shorthand_property_identifier_pattern" {
                    (text(part, source), None)
                } else {
                    continue;
                };
                facts.reference(
                    0,
                    EdgeKind::Imports,
                    format!("{module}::{name}"),
                    &scope.name,
                    alias,
                    lines(part),
                );
            }
        }
        _ => facts.reference(
            0,
            EdgeKind::Imports,
            module,
            &scope.name,
            binding.map(|n| text(n, source).into()),
            lines(node),
        ),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, TreeSitterParser};

    #[test]
    fn local_and_namespace_exports_preserve_aliases() {
        let facts = TreeSitterParser
            .parse(
                "x.js",
                b"export { local as default }; export * as helpers from './helpers';",
            )
            .unwrap();
        let actual = facts
            .references
            .iter()
            .map(|r| (r.kind, r.name.as_str(), r.alias.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                (EdgeKind::Reexports, "local", Some("default")),
                (EdgeKind::Reexports, "./helpers", Some("helpers"))
            ]
        );
    }

    #[test]
    fn dynamic_require_is_a_call_and_test_modifiers_keep_test_ownership() {
        let facts = TreeSitterParser
            .parse(
                "x.js",
                b"require(name); it.skip('case', () => worker.run());",
            )
            .unwrap();
        assert_eq!(
            facts
                .references
                .iter()
                .map(|r| (r.kind, r.name.as_str()))
                .collect::<Vec<_>>(),
            [
                (EdgeKind::Calls, "require"),
                (EdgeKind::Calls, "worker.run"),
                (EdgeKind::Tests, "worker.run")
            ]
        );
        assert_eq!(facts.references[1].src, facts.nodes[1].id);
    }
}
