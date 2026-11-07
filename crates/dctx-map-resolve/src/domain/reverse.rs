// SPDX-License-Identifier: Apache-2.0
//! Reverse index from exports and imports to the files that use them.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-B5 dependent invalidation.
//! Depends on resolved edges and raw references. Indexers rerun those files.
use super::{Facts, Resolution, ResolveError, Target, shared::join};
use dctx_map_model::EdgeKind;
use std::collections::{BTreeMap, BTreeSet};

/// Names whose change requires resolving other files again.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReverseIndex {
    /// `(package, qualified export)` → owner paths of edges into that export.
    pub dependents: BTreeMap<(String, String), BTreeSet<String>>,
    /// `(package, reference spelling)` → files that still name it.
    pub users: BTreeMap<(String, String), BTreeSet<String>>,
}

/// Indexes resolved edges and unresolved spellings for one snapshot.
/// The changed file is recorded too; invalidation drops that path later.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn reverse_index(files: &[Facts], resolution: &Resolution) -> ReverseIndex {
    let mut definitions = BTreeMap::new();
    for file in files {
        for definition in &file.definitions {
            definitions.insert(
                definition.node.id.clone(),
                (
                    file.package.clone(),
                    definition.qualified_name.clone(),
                    file.path.clone(),
                ),
            );
        }
    }
    let mut index = ReverseIndex::default();
    for edge in &resolution.edges {
        if let Some((package, qualified, defined_at)) = definitions.get(&edge.dst)
            && &edge.owner_path != defined_at
        {
            index
                .dependents
                .entry((package.clone(), qualified.clone()))
                .or_default()
                .insert(edge.owner_path.clone());
        }
    }
    for file in files {
        for reference in &file.references {
            for spelling in spellings(&reference.target) {
                index
                    .users
                    .entry((file.package.clone(), spelling))
                    .or_default()
                    .insert(file.path.clone());
            }
        }
    }
    index
}

/// Files other than the edited one that must be resolved again.
/// A body-only edit with the same exports and imports invalidates nobody.
/// An import edit invalidates users of every name that file contributes.
///
/// # Errors
/// Refuses snapshots whose path or package differs; they do not describe one edited file.
pub fn invalidated_dependents(
    index: &ReverseIndex,
    previous: &Facts,
    current: &Facts,
) -> Result<BTreeSet<String>, ResolveError> {
    if previous.path != current.path || previous.package != current.package {
        return Err(ResolveError::InvalidPath(current.path.clone()));
    }
    let before = contributed_names(previous);
    let after = contributed_names(current);
    let names: BTreeSet<String> = if import_set(previous) == import_set(current) {
        before.symmetric_difference(&after).cloned().collect()
    } else {
        before.union(&after).cloned().collect()
    };
    let mut invalidated = BTreeSet::new();
    for name in names {
        for bucket in [&index.users, &index.dependents] {
            if let Some(paths) = bucket.get(&(current.package.clone(), name.clone())) {
                for path in paths {
                    if path != &current.path {
                        invalidated.insert(path.clone());
                    }
                }
            }
        }
    }
    Ok(invalidated)
}

fn contributed_names(file: &Facts) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for definition in &file.definitions {
        names.insert(definition.qualified_name.clone());
        if let Some((_, bare)) = definition.qualified_name.rsplit_once("::") {
            names.insert(bare.to_owned());
        } else if !definition.qualified_name.is_empty() {
            names.insert(definition.qualified_name.clone());
        }
    }
    for reference in &file.references {
        if !matches!(reference.kind, EdgeKind::Imports | EdgeKind::Reexports) {
            continue;
        }
        for spelling in spellings(&reference.target) {
            names.insert(spelling);
        }
        let binding = binding_of(reference);
        if binding != "*" && !binding.is_empty() {
            names.insert(join(&reference.scope, &binding));
            names.insert(binding);
        }
    }
    names
}

fn import_set(file: &Facts) -> BTreeSet<(String, String, String, String)> {
    let mut imports = BTreeSet::new();
    for reference in &file.references {
        if matches!(reference.kind, EdgeKind::Imports | EdgeKind::Reexports) {
            imports.insert((
                reference.kind.as_str().to_owned(),
                reference.scope.clone(),
                binding_of(reference),
                target_token(&reference.target),
            ));
        }
    }
    imports
}

fn binding_of(reference: &super::Reference) -> String {
    if matches!(&reference.target, Target::Name(name) if name.ends_with("::*")) {
        return "*".into();
    }
    if let Some(binding) = &reference.binding {
        return binding.clone();
    }
    match &reference.target {
        Target::Name(name) => name.rsplit("::").next().unwrap_or("").to_owned(),
        Target::Id(id) => id.to_string(),
    }
}

fn target_token(target: &Target) -> String {
    match target {
        Target::Name(name) => name.clone(),
        Target::Id(id) => id.to_string(),
    }
}

fn spellings(target: &Target) -> Vec<String> {
    let raw = target_token(target);
    let mut names = vec![raw.clone()];
    if let Some(stripped) = raw.strip_prefix("crate::") {
        names.push(stripped.to_owned());
    }
    if let Some(stripped) = raw.strip_suffix("::*") {
        names.push(stripped.to_owned());
        if let Some(inner) = stripped.strip_prefix("crate::") {
            names.push(inner.to_owned());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::fixtures::{definition, file, reference};
    use crate::resolve;
    use dctx_map_model::NodeKind;

    #[test]
    fn changed_exports_and_imports_invalidate_only_reverse_dependents() {
        let worker = definition("src/worker.rs", "work", "worker::work", NodeKind::Symbol);
        let api = definition("src/api.rs", "api", "api", NodeKind::Module);
        let entry = definition("src/lib.rs", "entry", "entry", NodeKind::Symbol);
        let other = definition("src/other.rs", "other", "other", NodeKind::Symbol);
        let mut reexport = reference(&api, "api", EdgeKind::Reexports, "worker::work");
        reexport.binding = Some("execute".into());
        let mut api_file = file("src/api.rs", vec![api]);
        api_file.references.push(reexport);
        let mut lib = file("src/lib.rs", vec![entry.clone()]);
        lib.references
            .push(reference(&entry, "", EdgeKind::Imports, "api::execute"));
        lib.references
            .push(reference(&entry, "", EdgeKind::Calls, "worker::work"));
        lib.references
            .push(reference(&entry, "", EdgeKind::Calls, "lost::item"));
        let mut unrelated = file("src/other.rs", vec![other.clone()]);
        unrelated
            .references
            .push(reference(&other, "", EdgeKind::Calls, "other"));
        let worker_file = file("src/worker.rs", vec![worker.clone()]);
        let mut files = vec![worker_file.clone(), api_file.clone(), lib, unrelated];
        let resolution = resolve(&files).unwrap();
        let index = reverse_index(&files, &resolution);
        files.reverse();
        assert_eq!(reverse_index(&files, &resolve(&files).unwrap()), index);

        let mut renamed = worker_file.clone();
        renamed.definitions[0].qualified_name = "worker::job".into();
        renamed.definitions[0].node.name = "job".into();
        let lost = invalidated_dependents(&index, &worker_file, &renamed).unwrap();
        assert!(lost.contains("src/lib.rs"));
        assert!(!lost.contains("src/other.rs"));
        assert!(!lost.contains("src/worker.rs"));

        let mut rebound = api_file.clone();
        rebound.references[0].binding = Some("go".into());
        let rebound_lost = invalidated_dependents(&index, &api_file, &rebound).unwrap();
        assert!(rebound_lost.contains("src/lib.rs"));
        assert!(!rebound_lost.contains("src/other.rs"));

        let mut fingerprint = worker_file.clone();
        fingerprint.definitions[0].node.fingerprint.push('x');
        assert!(
            invalidated_dependents(&index, &worker_file, &fingerprint)
                .unwrap()
                .is_empty()
        );

        let added = file(
            "src/lost.rs",
            vec![definition(
                "src/lost.rs",
                "item",
                "lost::item",
                NodeKind::Symbol,
            )],
        );
        let empty = file("src/lost.rs", vec![]);
        let found = invalidated_dependents(&index, &empty, &added).unwrap();
        assert!(found.contains("src/lib.rs"));
        assert!(!found.contains("src/other.rs"));
        let unrelated_export = file(
            "src/lost.rs",
            vec![definition(
                "src/lost.rs",
                "nope",
                "lost::nope",
                NodeKind::Symbol,
            )],
        );
        assert!(
            !invalidated_dependents(&index, &empty, &unrelated_export)
                .unwrap()
                .contains("src/lib.rs")
        );
    }
}
