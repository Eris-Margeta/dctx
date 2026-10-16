//! Parallel resolution of independent package components.
//! Implements SPEC-MAP-MODE section 6.3 without changing resolver semantics.
//! Explicit cross-package references join components; document paths import only their target.
use crate::{Error, Result};
use dctx_map_model::{EdgeKind, NodeKind, StableId};
use dctx_map_resolve::{Facts, Resolution, Target};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};

fn root(parents: &BTreeMap<String, String>, package: &str) -> String {
    let mut current = package;
    while let Some(parent) = parents.get(current) {
        if parent == current {
            break;
        }
        current = parent;
    }
    current.to_owned()
}
pub(super) fn resolve_components(mut files: Vec<Facts>) -> Result<Resolution> {
    let owners: BTreeMap<_, _> = files
        .iter()
        .flat_map(|f| {
            f.definitions
                .iter()
                .map(move |d| (d.node.id.clone(), f.package.clone()))
        })
        .collect();
    let modules: BTreeMap<_, _> = files
        .iter()
        .flat_map(|f| {
            f.definitions
                .iter()
                .filter(|d| d.node.kind == NodeKind::Module)
                .map(move |d| (d.node.path.clone(), d.node.id.clone()))
        })
        .collect();
    let mut parents: BTreeMap<_, _> = files
        .iter()
        .map(|f| (f.package.clone(), f.package.clone()))
        .collect();
    for file in &mut files {
        for reference in &mut file.references {
            if reference.kind == EdgeKind::Documents {
                if let Target::Name(name) = &reference.target
                    && let Some(id) = modules.get(name)
                {
                    reference.target = Target::Id(id.clone());
                }
                continue;
            }
            if let Target::Id(id) = &reference.target
                && let Some(package) = owners.get(id)
            {
                let a = root(&parents, &file.package);
                let b = root(&parents, package);
                if a != b {
                    parents.insert(b, a);
                }
            }
        }
    }
    let mut groups: BTreeMap<String, Vec<Facts>> = BTreeMap::new();
    for file in &files {
        groups
            .entry(root(&parents, &file.package))
            .or_default()
            .push(file.clone());
    }
    let definitions: BTreeMap<_, _> = files
        .iter()
        .flat_map(|file| {
            file.definitions
                .iter()
                .map(move |d| (d.node.id.clone(), (file, d)))
        })
        .collect();
    for group in groups.values_mut() {
        let owned: BTreeSet<_> = group
            .iter()
            .flat_map(|f| f.definitions.iter().map(|d| d.node.id.clone()))
            .collect();
        let targets: BTreeSet<StableId> = group
            .iter()
            .flat_map(|f| &f.references)
            .filter_map(|r| match &r.target {
                Target::Id(id) if r.kind == EdgeKind::Documents && !owned.contains(id) => {
                    Some(id.clone())
                }
                _ => None,
            })
            .collect();
        let mut extra: BTreeMap<String, Facts> = BTreeMap::new();
        for id in targets {
            if let Some((file, definition)) = definitions.get(&id) {
                extra
                    .entry(file.path.clone())
                    .or_insert_with(|| Facts {
                        path: file.path.clone(),
                        package: file.package.clone(),
                        definitions: vec![],
                        references: vec![],
                    })
                    .definitions
                    .push((*definition).clone());
            }
        }
        group.extend(extra.into_values());
    }
    let groups: Vec<_> = groups.into_values().collect();
    let cursor = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
        .min(groups.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut result = Resolution::default();
                    while let Some(group) = groups.get(cursor.fetch_add(1, Ordering::Relaxed)) {
                        let resolved = dctx_map_resolve::resolve(group)?;
                        result.edges.extend(resolved.edges);
                        result.unresolved.extend(resolved.unresolved);
                    }
                    Ok::<_, dctx_map_resolve::ResolveError>(result)
                })
            })
            .collect();
        let mut result = Resolution::default();
        for handle in handles {
            let resolved = handle
                .join()
                .map_err(|_| Error::Request("resolver worker failed".into()))??;
            result.edges.extend(resolved.edges);
            result.unresolved.extend(resolved.unresolved);
        }
        result.edges.sort_by(|a, b| {
            (&a.owner_path, &a.src, a.kind, &a.dst, a.conf).cmp(&(
                &b.owner_path,
                &b.src,
                b.kind,
                &b.dst,
                b.conf,
            ))
        });
        result.edges.dedup();
        result.unresolved.sort();
        result.unresolved.dedup();
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{bridge::bridge, cached::CachedFile};
    use dctx_map_parse::{Parser, TreeSitterParser};
    #[test]
    fn components_preserve_monolithic_resolution_including_cross_package_docs() {
        let sources = [
            ("a/Cargo.toml", ""),
            ("b/Cargo.toml", ""),
            (
                "a/src/lib.rs",
                "pub fn target() {} pub fn caller() { target(); }",
            ),
            ("b/src/lib.rs", "pub fn other() { a::target(); }"),
            ("notes.md", "# Notes\nSee `a/src/lib.rs`."),
        ];
        let files: BTreeMap<_, _> = sources
            .into_iter()
            .map(|(path, text)| {
                (
                    path.to_owned(),
                    CachedFile::parsed(
                        TreeSitterParser.parse(path, text.as_bytes()).unwrap(),
                        String::new(),
                        text.len() as u64,
                        0,
                        true,
                    ),
                )
            })
            .collect();
        let facts = bridge(&files).unwrap().facts;
        assert_eq!(
            resolve_components(facts.clone()).unwrap(),
            dctx_map_resolve::resolve(&facts).unwrap()
        );
    }
}
