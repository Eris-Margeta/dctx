//! Deterministic conversion of syntactic references into file-owned edges.
//! Implements SPEC-MAP-MODE sections 5.3 and 6.3, retaining retry inputs.
//! Uses the symbol table and model confidence matrix; called by indexers.
use super::{Facts, Resolution, ResolveError, SymbolTable, Unresolved, UnresolvedReason};
use dctx_map_model::{Confidence, Edge, EdgeKind, Node, NodeKind, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};

/// Resolves a complete snapshot; neither input order nor duplicate references
/// affect the result. Missing and ambiguous references remain lossless retry facts.
pub fn resolve(files: &[Facts]) -> Result<Resolution, ResolveError> {
    let table = SymbolTable::new(files)?;
    let mut edges = BTreeMap::new();
    let mut unresolved = BTreeSet::new();
    for file in files {
        for reference in &file.references {
            let mut found = table.lookup(&file.package, reference);
            found.candidates.retain(|id, _| {
                table
                    .node(id)
                    .is_some_and(|node| compatible(reference.kind, node))
            });
            if found.candidates.len() != 1 {
                let reason = if found.candidates.is_empty() {
                    UnresolvedReason::Missing
                } else {
                    UnresolvedReason::Ambiguous(found.candidates.into_keys().collect())
                };
                unresolved.insert(Unresolved {
                    owner_path: file.path.clone(),
                    reference: reference.clone(),
                    reason,
                });
                continue;
            }
            for (destination, exact) in found.candidates {
                let confidence = confidence(reference.kind, exact, found.scoped);
                let key = (
                    file.path.clone(),
                    reference.source.clone(),
                    reference.kind,
                    destination,
                );
                edges
                    .entry(key)
                    .and_modify(|current: &mut Confidence| {
                        *current = (*current).min(confidence);
                    })
                    .or_insert(confidence);
            }
        }
    }
    Ok(Resolution {
        edges: edges
            .into_iter()
            .map(|((owner_path, src, kind, dst), conf)| Edge {
                src,
                dst,
                kind,
                conf,
                owner_path,
            })
            .collect(),
        unresolved: unresolved.into_iter().collect(),
    })
}

fn confidence(kind: EdgeKind, exact: bool, scoped: bool) -> Confidence {
    match kind {
        EdgeKind::Contains => Confidence::Exact,
        EdgeKind::Calls if !scoped => Confidence::Guess,
        EdgeKind::Imports | EdgeKind::Implements | EdgeKind::Reexports if exact => {
            Confidence::Exact
        }
        _ => Confidence::Name,
    }
}

fn compatible(kind: EdgeKind, node: &Node) -> bool {
    match kind {
        EdgeKind::Contains | EdgeKind::Documents => true,
        EdgeKind::Imports | EdgeKind::Reexports => {
            matches!(node.kind, NodeKind::Module | NodeKind::Symbol)
        }
        EdgeKind::Calls | EdgeKind::Tests => node.kind == NodeKind::Symbol,
        EdgeKind::Implements => {
            node.kind == NodeKind::Symbol
                && matches!(
                    node.symbol_kind,
                    None | Some(SymbolKind::Trait | SymbolKind::Interface)
                )
        }
        EdgeKind::UsesType => {
            node.kind == NodeKind::Symbol
                && matches!(
                    node.symbol_kind,
                    None | Some(
                        SymbolKind::Struct
                            | SymbolKind::Enum
                            | SymbolKind::Trait
                            | SymbolKind::Type
                            | SymbolKind::Class
                            | SymbolKind::Interface
                    )
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Target, fixtures::*};
    use proptest::prelude::*;

    #[test]
    fn three_module_fixture_has_the_exact_expected_edges_and_confidences() {
        let fixture = three_modules();
        let result = resolve(&fixture.files).unwrap();
        assert_eq!(result.edges, fixture.expected);
        assert_eq!(result.unresolved.len(), 1);
        assert_eq!(
            result.unresolved[0].reference.target,
            Target::Name("external::missing".into())
        );
        assert_eq!(result.unresolved[0].reason, UnresolvedReason::Missing);
        assert!(result.edges.iter().all(|edge| edge.kind.permits(edge.conf)));
    }

    proptest! {
        #[test]
        fn file_definition_and_reference_order_do_not_change_resolution(
            swaps in prop::collection::vec(any::<usize>(), 0..80),
        ) {
            let mut fixture = three_modules();
            let expected = resolve(&fixture.files).unwrap();
            for (index, key) in swaps.iter().enumerate() {
                let count = fixture.files.len();
                fixture.files.swap(index % count, key % count);
                let file = &mut fixture.files[index % count];
                if !file.definitions.is_empty() {
                    let count = file.definitions.len();
                    file.definitions.swap(index % count, key % count);
                }
                if !file.references.is_empty() {
                    let count = file.references.len();
                    file.references.swap(index % count, key % count);
                }
            }
            prop_assert_eq!(resolve(&fixture.files).unwrap(), expected);
        }
    }

    #[test]
    fn ambiguity_is_preserved_and_unresolved_can_be_retried() {
        let source = definition("lib.rs", "caller", "caller", NodeKind::Symbol);
        let a = definition("a.rs", "work", "a::work", NodeKind::Symbol);
        let b = definition("b.rs", "work", "b::work", NodeKind::Symbol);
        let mut root = file("lib.rs", vec![source.clone()]);
        root.references
            .push(reference(&source, "", EdgeKind::Calls, "work"));
        let mut files = vec![root];
        let missing = resolve(&files).unwrap();
        assert!(missing.edges.is_empty());
        assert_eq!(missing.unresolved[0].reason, UnresolvedReason::Missing);
        files.push(file("a.rs", vec![a.clone()]));
        let unique = resolve(&files).unwrap();
        assert!(unique.unresolved.is_empty());
        assert_eq!(unique.edges[0].conf, Confidence::Guess);
        files.push(file("b.rs", vec![b.clone()]));
        let ambiguous = resolve(&files).unwrap();
        assert!(ambiguous.edges.is_empty());
        assert_eq!(
            ambiguous.unresolved[0].reference,
            missing.unresolved[0].reference
        );
        assert_eq!(
            ambiguous.unresolved[0].reason,
            UnresolvedReason::Ambiguous(vec![a.node.id, b.node.id])
        );
    }

    #[test]
    fn package_isolation_explicit_ids_and_strongest_edge_deduplication() {
        let source = definition("lib.rs", "caller", "caller", NodeKind::Symbol);
        let target = definition("other.rs", "work", "other::work", NodeKind::Symbol);
        let mut root = file("lib.rs", vec![source.clone()]);
        root.references
            .push(reference(&source, "", EdgeKind::Calls, "work"));
        let mut other = file("other.rs", vec![target.clone()]);
        other.package = "other-package".into();
        assert_eq!(
            resolve(&[root.clone(), other.clone()])
                .unwrap()
                .unresolved
                .len(),
            1
        );
        let mut direct = root.references[0].clone();
        direct.target = Target::Id(target.node.id.clone());
        root.references.extend([direct.clone(), direct]);
        other.package = root.package.clone();
        let result = resolve(&[root, other]).unwrap();
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].conf, Confidence::Name);
    }

    #[test]
    fn incompatible_types_and_missing_explicit_targets_remain_unresolved() {
        let source = definition("a.rs", "run", "run", NodeKind::Symbol);
        let mut root = file("a.rs", vec![source.clone()]);
        root.references
            .push(reference(&source, "", EdgeKind::UsesType, "run"));
        let mut missing = reference(&source, "", EdgeKind::Contains, "unused");
        missing.target = Target::Id("s:a.rs#missing".parse().unwrap());
        root.references.push(missing);
        let result = resolve(&[root]).unwrap();
        assert!(result.edges.is_empty());
        assert_eq!(result.unresolved.len(), 2);
        assert!(
            result
                .unresolved
                .iter()
                .all(|item| item.reason == UnresolvedReason::Missing)
        );
    }
}
