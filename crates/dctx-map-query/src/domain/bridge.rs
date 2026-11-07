// SPDX-License-Identifier: Apache-2.0
//! Package-aware conversion between parser facts and resolver inputs.
//! Implements SPEC-MAP-MODE sections 5.1, 5.3 and 6.3.
//! Uses only public map types and creates the place/package containment graph.
use super::{cached::CachedFile, inside};
use crate::Result;
use dctx_map_model::{
    Confidence, Edge, EdgeKind, Node, NodeKind, StableId, Summary, SummarySource,
};
use dctx_map_resolve::{Definition, Facts, Reference, Target};
use std::collections::{BTreeMap, BTreeSet};

fn join(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.into(),
        (_, true) => a.into(),
        _ => format!("{a}::{b}"),
    }
}
fn canonical(value: &str) -> String {
    value
        .split("::")
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut clean: String = s
                .chars()
                .map(|c| {
                    if c.is_whitespace() || c.is_control() || matches!(c, ':' | '/' | '\\') {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            if matches!(clean.as_str(), "crate" | "self" | "super" | "*") {
                clean.insert(0, '_');
            }
            clean
        })
        .collect::<Vec<_>>()
        .join("::")
}
fn module(path: &str, package: &str) -> String {
    let relative = if package == "." {
        path
    } else {
        path.strip_prefix(package)
            .unwrap_or(path)
            .trim_start_matches('/')
    };
    let relative = relative
        .strip_prefix("src/")
        .unwrap_or(relative)
        .trim_end_matches(".rs");
    let mut parts: Vec<_> = relative.split('/').collect();
    if parts
        .last()
        .is_some_and(|p| matches!(*p, "lib" | "main" | "mod"))
    {
        parts.pop();
    }
    canonical(&parts.join("::"))
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(p, _)| p)
}
fn package_for<'a>(path: &str, packages: &'a BTreeSet<String>) -> &'a str {
    packages
        .iter()
        .filter(|p| inside(path, p))
        .max_by_key(|p| p.len())
        .map_or(".", String::as_str)
}
fn synthetic(kind: NodeKind, path: &str, name: &str) -> Result<Node> {
    Ok(Node {
        id: StableId::new(kind, path, None, None)?,
        kind,
        symbol_kind: None,
        path: path.into(),
        lines: [1, 1],
        name: name.into(),
        signature: String::new(),
        fingerprint: String::new(),
        public: true,
        summary: Summary {
            text: if kind == NodeKind::Place {
                "Place root".into()
            } else {
                format!("Package {name}")
            },
            source: SummarySource::Doc,
        },
    })
}
fn contains(src: &StableId, dst: &StableId, owner: &str) -> Edge {
    Edge {
        src: src.clone(),
        dst: dst.clone(),
        kind: EdgeKind::Contains,
        conf: Confidence::Exact,
        owner_path: owner.into(),
    }
}

pub(crate) struct Bridged {
    pub facts: Vec<Facts>,
    pub structural: Vec<Node>,
    pub edges: Vec<Edge>,
}
#[allow(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "Preserve the existing literal suffix policy for source language dispatch and privacy exclusions; case folding would change which paths are parsed or denied."
)]
pub(crate) fn bridge(files: &BTreeMap<String, CachedFile>) -> Result<Bridged> {
    let packages: BTreeSet<_> = files
        .keys()
        .filter(|p| {
            p.ends_with("/Cargo.toml")
                || *p == "Cargo.toml"
                || p.ends_with("/package.json")
                || *p == "package.json"
        })
        .map(|p| parent(p).to_owned())
        .collect();
    let root = synthetic(NodeKind::Place, ".", ".")?;
    let mut structural = vec![root.clone()];
    let mut edges = Vec::new();
    for package in &packages {
        let node = synthetic(
            NodeKind::Package,
            package,
            package.rsplit('/').next().unwrap_or(package),
        )?;
        edges.push(contains(&root.id, &node.id, package));
        structural.push(node);
    }
    let mut facts = Vec::new();
    for (path, file) in files {
        let package = package_for(path, &packages);
        let prefix = if path.ends_with(".rs") {
            module(path, package)
        } else {
            canonical(path)
        };
        let definitions = file
            .nodes
            .iter()
            .map(|node| Definition {
                qualified_name: if node.kind == NodeKind::Module {
                    prefix.clone()
                } else {
                    join(&prefix, &canonical(&node.name))
                },
                node: node.clone(),
            })
            .collect();
        edges.extend(file.edges.iter().cloned());
        let references = file
            .references
            .iter()
            .map(|reference| Reference {
                source: reference.source.clone(),
                kind: reference.kind,
                target: Target::Name(reference.name.clone()),
                scope: join(&prefix, &canonical(&reference.scope)),
                binding: reference.binding.clone(),
            })
            .collect();
        facts.push(Facts {
            path: path.clone(),
            package: package.into(),
            definitions,
            references,
        });
        if let Some(node) = file.nodes.iter().find(|n| n.kind == NodeKind::Module) {
            let container = if packages.contains(package) {
                StableId::new(NodeKind::Package, package, None, None)?
            } else {
                root.id.clone()
            };
            edges.push(contains(&container, &node.id, path));
        }
    }
    // Cross-package names are accepted only when one exact definition exists.
    let mut external: BTreeMap<String, Vec<StableId>> = BTreeMap::new();
    for file in &facts {
        let name = file
            .package
            .rsplit('/')
            .next()
            .unwrap_or("")
            .replace('-', "_");
        for definition in &file.definitions {
            external
                .entry(join(&name, &definition.qualified_name))
                .or_default()
                .push(definition.node.id.clone());
        }
    }
    for file in &mut facts {
        for reference in &mut file.references {
            if let Target::Name(name) = &reference.target
                && let Some(ids) = external.get(name)
                && ids.len() == 1
            {
                reference.target = Target::Id(ids[0].clone());
            }
        }
    }
    Ok(Bridged {
        facts,
        structural,
        edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modules_are_package_relative_and_mod_is_not_a_scope() {
        assert_eq!(module("crates/a/src/lib.rs", "crates/a"), "");
        assert_eq!(module("crates/a/src/domain/mod.rs", "crates/a"), "domain");
        assert_eq!(
            module("crates/a/src/domain/item.rs", "crates/a"),
            "domain::item"
        );
    }
}
