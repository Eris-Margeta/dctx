// SPDX-License-Identifier: Apache-2.0
//! Explicit alias, name and body collisions.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-B5 disambiguation.
//! Depends on facts, nodes and stable ids. Resolution never picks a winner.
use super::{Facts, Reference, Target};
use dctx_map_model::{Node, NodeKind, StableId, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};

/// Two imports in one scope introduce the same local name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AliasCollision {
    /// Package that contains the scope.
    pub package: String,
    /// Package-relative scope of the imports.
    pub scope: String,
    /// Local name both imports claim.
    pub binding: String,
    /// Competing spellings or identities, sorted and unique.
    pub targets: Vec<String>,
}

/// One display name used by more than one node in a package.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NameCollision {
    /// Package that contains the candidates.
    pub package: String,
    /// Display or bare name that collided.
    pub name: String,
    /// Every matching identity, sorted. Never reduced to one.
    pub candidates: Vec<StableId>,
}

/// Several disappeared or added bodies share one fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BodyCollision {
    /// Shared nonempty body fingerprint.
    pub fingerprint: String,
    /// Node category of the group.
    pub kind: NodeKind,
    /// Symbol category of the group, when present.
    pub symbol_kind: Option<SymbolKind>,
    /// Disappeared identities, sorted.
    pub old: Vec<StableId>,
    /// Newly introduced identities, sorted.
    pub new: Vec<StableId>,
}

/// Result of expanding a full id or a short alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AliasExpansion {
    /// The query names one identity.
    Unique(StableId),
    /// The query names several identities. Callers must not drop any.
    Collision(Vec<StableId>),
    /// Nothing in the snapshot matches.
    Unknown,
}

/// Import bindings that compete. A single binding is not a collision.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn alias_collisions(files: &[Facts]) -> Vec<AliasCollision> {
    let mut groups: BTreeMap<(String, String, String), BTreeSet<String>> = BTreeMap::new();
    for file in files {
        for reference in &file.references {
            if !matches!(
                reference.kind,
                dctx_map_model::EdgeKind::Imports | dctx_map_model::EdgeKind::Reexports
            ) {
                continue;
            }
            let Some(binding) = introduced_name(reference) else {
                continue;
            };
            groups
                .entry((file.package.clone(), reference.scope.clone(), binding))
                .or_default()
                .insert(target_token(&reference.target));
        }
    }
    groups
        .into_iter()
        .filter(|(_, targets)| targets.len() > 1)
        .map(|((package, scope, binding), targets)| AliasCollision {
            package,
            scope,
            binding,
            targets: targets.into_iter().collect(),
        })
        .collect()
}

/// Display names that belong to more than one node.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn name_collisions(files: &[Facts]) -> Vec<NameCollision> {
    grouped_names(files)
        .into_iter()
        .filter(|collision| collision.candidates.len() > 1)
        .collect()
}

/// Every node with this display or bare name, sorted by identity.
/// One match returns that match. Several matches return all of them.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn disambiguate_name(files: &[Facts], package: &str, name: &str) -> Vec<StableId> {
    grouped_names(files)
        .into_iter()
        .find(|collision| collision.package == package && collision.name == name)
        .map(|collision| collision.candidates)
        .unwrap_or_default()
}

fn grouped_names(files: &[Facts]) -> Vec<NameCollision> {
    let mut groups: BTreeMap<(String, String), BTreeSet<StableId>> = BTreeMap::new();
    for file in files {
        for definition in &file.definitions {
            if definition.node.name.is_empty() {
                continue;
            }
            groups
                .entry((file.package.clone(), definition.node.name.clone()))
                .or_default()
                .insert(definition.node.id.clone());
            let bare = definition
                .qualified_name
                .rsplit("::")
                .next()
                .unwrap_or(definition.qualified_name.as_str());
            if bare != definition.node.name && !bare.is_empty() {
                groups
                    .entry((file.package.clone(), bare.to_owned()))
                    .or_default()
                    .insert(definition.node.id.clone());
            }
        }
    }
    groups
        .into_iter()
        .map(|((package, name), candidates)| NameCollision {
            package,
            name,
            candidates: candidates.into_iter().collect(),
        })
        .collect()
}

/// Classifies matches that were already collected.
/// Zero is unknown, one is unique, and the rest stay a sorted collision.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn expansion_from_hits(mut hits: Vec<StableId>) -> AliasExpansion {
    hits.sort();
    hits.dedup();
    match hits.len() {
        0 => AliasExpansion::Unknown,
        1 => AliasExpansion::Unique(hits.remove(0)),
        _ => AliasExpansion::Collision(hits),
    }
}

/// Expands a full stable id or a short `@` alias. Colliding aliases stay lists.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn expand_alias(ids: &[StableId], query: &str) -> AliasExpansion {
    if let Ok(id) = query.parse::<StableId>() {
        return if ids.iter().any(|candidate| candidate == &id) {
            AliasExpansion::Unique(id)
        } else {
            AliasExpansion::Unknown
        };
    }
    let hits = ids
        .iter()
        .filter(|id| id.alias().as_deref() == Some(query))
        .cloned()
        .collect();
    expansion_from_hits(hits)
}

/// Body groups that are not a single disappeared-to-added move.
/// Empty fingerprints and unique pairs are omitted. `detect_moves` keeps the pairs.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn body_collisions(previous: &[Node], current: &[Node]) -> Vec<BodyCollision> {
    let old_ids: BTreeSet<_> = previous.iter().map(|node| &node.id).collect();
    let new_ids: BTreeSet<_> = current.iter().map(|node| &node.id).collect();
    let removed = group(previous, &new_ids);
    let added = group(current, &old_ids);
    let fingerprints: BTreeSet<_> = removed.keys().chain(added.keys()).cloned().collect();
    let mut collisions = Vec::new();
    for fingerprint in &fingerprints {
        let old = removed.get(fingerprint).cloned().unwrap_or_default();
        let new = added.get(fingerprint).cloned().unwrap_or_default();
        if old.is_empty() || new.is_empty() || (old.len() == 1 && new.len() == 1) {
            continue;
        }
        collisions.push(BodyCollision {
            fingerprint: fingerprint.2.clone(),
            kind: fingerprint.0,
            symbol_kind: fingerprint.1,
            old,
            new,
        });
    }
    collisions.sort();
    collisions
}

type Print = (NodeKind, Option<SymbolKind>, String);

fn group(nodes: &[Node], retained: &BTreeSet<&StableId>) -> BTreeMap<Print, Vec<StableId>> {
    let mut groups: BTreeMap<Print, Vec<StableId>> = BTreeMap::new();
    for node in nodes {
        if node.fingerprint.is_empty() || retained.contains(&node.id) {
            continue;
        }
        groups
            .entry((node.kind, node.symbol_kind, node.fingerprint.clone()))
            .or_default()
            .push(node.id.clone());
    }
    for ids in groups.values_mut() {
        ids.sort();
    }
    groups
}

fn introduced_name(reference: &Reference) -> Option<String> {
    if matches!(&reference.target, Target::Name(name) if name.ends_with("::*")) {
        return None;
    }
    if let Some(binding) = &reference.binding {
        return Some(binding.clone());
    }
    match &reference.target {
        Target::Name(name) => name.rsplit("::").next().map(str::to_owned),
        Target::Id(id) => Some(id.to_string()),
    }
}

fn target_token(target: &Target) -> String {
    match target {
        Target::Name(name) => name.clone(),
        Target::Id(id) => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect_moves;
    use crate::domain::fixtures::{definition, file, reference};
    use dctx_map_model::EdgeKind;

    #[test]
    fn alias_and_name_collisions_list_every_candidate() {
        let left = definition("src/a.rs", "work", "a::work", NodeKind::Symbol);
        let right = definition("src/b.rs", "work", "b::work", NodeKind::Symbol);
        let entry = definition("src/lib.rs", "entry", "entry", NodeKind::Symbol);
        let mut root = file("src/lib.rs", vec![entry.clone()]);
        for name in ["crate::a::work", "crate::b::work"] {
            let mut import = reference(&entry, "", EdgeKind::Imports, name);
            import.binding = Some("execute".into());
            root.references.push(import);
        }
        let mut files = vec![
            root,
            file("src/a.rs", vec![left.clone()]),
            file("src/b.rs", vec![right.clone()]),
        ];
        let collisions = alias_collisions(&files);
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0].binding, "execute");
        assert_eq!(collisions[0].targets.len(), 2);
        files[0].references.reverse();
        files.reverse();
        assert_eq!(alias_collisions(&files), collisions);
        let alone = file(
            "src/lib.rs",
            vec![entry.clone()],
        );
        assert_eq!(alias_collisions(std::slice::from_ref(&alone)), [] as [crate::domain::collision::AliasCollision; 0]);
        let names = disambiguate_name(&files, "fixture", "work");
        assert_eq!(names, vec![left.node.id.clone(), right.node.id.clone()]);
        assert_eq!(name_collisions(&files)[0].candidates, names);
        assert_eq!(
            disambiguate_name(&files, "fixture", "entry"),
            vec![entry.node.id]
        );
    }

    #[test]
    fn alias_expansion_and_body_groups_do_not_drop_matches() {
        let path = format!("f/{}", "a".repeat(130));
        let id = StableId::new(NodeKind::Symbol, &path, Some("n"), None).unwrap();
        let alias = id.alias().unwrap();
        assert_eq!(
            expand_alias(std::slice::from_ref(&id), &alias),
            AliasExpansion::Unique(id.clone())
        );
        assert_eq!(
            expand_alias(std::slice::from_ref(&id), &alias),
            expansion_from_hits(vec![id.clone()])
        );
        assert!(matches!(
            expand_alias(std::slice::from_ref(&id), "@zzzzzz"),
            AliasExpansion::Unknown
        ));
        assert!(matches!(
            expand_alias(&[], "s:missing.rs#n"),
            AliasExpansion::Unknown
        ));
        let other = StableId::new(NodeKind::Symbol, "src/b.rs", Some("m"), None).unwrap();
        let collision = expansion_from_hits(vec![other.clone(), id.clone()]);
        let mut expected = vec![other.clone(), id];
        expected.sort();
        assert_eq!(collision, AliasExpansion::Collision(expected));

        let mut old = definition("src/old.rs", "calculate", "old::calculate", NodeKind::Symbol).node;
        old.fingerprint = "same-body".into();
        let mut new = definition("src/new.rs", "compute", "new::compute", NodeKind::Symbol).node;
        new.fingerprint = old.fingerprint.clone();
        assert_eq!(body_collisions(std::slice::from_ref(&old), std::slice::from_ref(&new)), [] as [crate::domain::collision::BodyCollision; 0]);
        assert_eq!(detect_moves(&[old.clone()], &[new.clone()], "r").len(), 1);
        let mut another = new.clone();
        another.id = other;
        let mut previous = vec![old.clone()];
        let mut current = vec![new.clone(), another.clone()];
        let collisions = body_collisions(&previous, &current);
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0].old, vec![old.id.clone()]);
        assert_eq!(collisions[0].new, vec![another.id.clone(), new.id.clone()]);
        assert_eq!(detect_moves(&previous, &current, "r"), [] as [crate::domain::moves::Move; 0]);
        previous.reverse();
        current.reverse();
        assert_eq!(body_collisions(&previous, &current), collisions);
        old.fingerprint.clear();
        new.fingerprint.clear();
        another.fingerprint.clear();
        assert_eq!(body_collisions(&[old], &[new, another]), [] as [crate::domain::collision::BodyCollision; 0]);
    }
}
