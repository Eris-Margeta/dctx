// SPDX-License-Identifier: Apache-2.0
//! A crate's module declarations establish the files eligible to satisfy a use tree.
use super::{EdgeKind, HashMap, HashSet, RawRef, RefForm, Tables, dir_of};
use crate::index::refs_names::inventory_join;

#[derive(Default)]
pub(super) struct ModuleTree {
    /// File to crate and logical module prefix. Multiple contexts require configuration.
    owners: HashMap<u32, Vec<(u32, String)>>,
    modules: HashMap<(u32, String), u32>,
}

fn qualified(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else if child.is_empty() {
        parent.to_owned()
    } else {
        format!("{parent}::{child}")
    }
}

impl ModuleTree {
    pub(super) fn build(t: &Tables<'_>) -> Self {
        let mut tree = Self::default();
        for (&path, &root) in &t.module_paths {
            if !matches!(path.rsplit('/').next(), Some("lib.rs" | "main.rs")) {
                continue;
            }
            let mut pending = vec![(root, String::new(), dir_of(path).to_owned())];
            let mut seen = HashSet::new();
            while let Some((file, prefix, dir)) = pending.pop() {
                tree.owners
                    .entry(file)
                    .or_default()
                    .push((root, prefix.clone()));
                if !seen.insert(file) {
                    continue;
                }
                tree.modules.insert((root, prefix.clone()), file);
                for r in t.facts(file) {
                    let RefForm::RustModule { path: overridden } = &r.form else {
                        continue;
                    };
                    let scope = r.scope.replace("::", "/");
                    let base = if scope.is_empty() {
                        dir.clone()
                    } else {
                        format!("{dir}/{scope}").trim_start_matches('/').to_owned()
                    };
                    let hit = if let Some(operand) = overridden {
                        let source_dir = if r.scope.is_empty() {
                            dir_of(t.text.get(t.nodes[file as usize].path))
                        } else {
                            &base
                        };
                        inventory_join(source_dir, operand)
                            .and_then(|p| t.module_paths.get(p.as_str()).copied())
                    } else {
                        let Some(stem) = inventory_join(&base, &r.name) else {
                            continue;
                        };
                        match (
                            t.module_paths.get(format!("{stem}.rs").as_str()),
                            t.module_paths.get(format!("{stem}/mod.rs").as_str()),
                        ) {
                            (Some(&m), None) | (None, Some(&m)) => Some(m),
                            _ => None,
                        }
                    };
                    if let Some(child) = hit {
                        let path = t.text.get(t.nodes[child as usize].path);
                        let next = path
                            .strip_suffix("/mod.rs")
                            .or_else(|| path.strip_suffix(".rs"))
                            .unwrap_or(path);
                        pending.push((
                            child,
                            qualified(&qualified(&prefix, &r.scope), &r.name),
                            next.into(),
                        ));
                    }
                }
            }
        }
        tree
    }
}

impl Tables<'_> {
    pub(super) fn rust_target(&self, importer: &str, r: &RawRef) -> Option<(u32, bool)> {
        let file = *self.module_paths.get(importer)?;
        let [(root, prefix)] = self.rust.owners.get(&file)?.as_slice() else {
            return None;
        };
        if matches!(r.form, RefForm::RustModule { .. }) {
            let key = qualified(&qualified(prefix, &r.scope), &r.name);
            return self
                .rust
                .modules
                .get(&(*root, key))
                .copied()
                .map(|m| (m, true));
        }
        self.rust_use(*root, file, prefix, r, &mut Vec::new())
    }

    fn rust_use(
        &self,
        root: u32,
        file: u32,
        prefix: &str,
        r: &RawRef,
        visiting: &mut Vec<(u32, String)>,
    ) -> Option<(u32, bool)> {
        let mut path = r.name.split("::").peekable();
        let inline = self
            .facts(file)
            .iter()
            .filter(|fact| {
                fact.form == RefForm::Namespace
                    && (r.scope == fact.name
                        || r.scope
                            .strip_prefix(&fact.name)
                            .is_some_and(|s| s.starts_with("::")))
            })
            .max_by_key(|fact| fact.name.len())
            .map_or("", |fact| fact.name.as_str());
        let origin = qualified(prefix, inline);
        let mut context = origin.clone();
        match path.peek().copied()? {
            "crate" => {
                path.next();
                context.clear();
            }
            "self" => {
                path.next();
            }
            "super" => {
                while path.next_if_eq(&"super").is_some() {
                    if context.is_empty() {
                        return None;
                    }
                    context = context.rsplit_once("::").map_or("", |(p, _)| p).into();
                }
            }
            "" => return None,
            _ => context.clear(),
        }
        let key = qualified(&context, &path.collect::<Vec<_>>().join("::"));
        if let Some(&m) = self.rust.modules.get(&(root, key.clone())) {
            return Some((m, true));
        }
        let mut owner = key.as_str();
        loop {
            owner = owner.rsplit_once("::").map_or("", |(p, _)| p);
            if let Some(&m) = self.rust.modules.get(&(root, owner.into())) {
                let local = key.strip_prefix(owner)?.trim_start_matches("::");
                let (scope, name) = local.rsplit_once("::").unwrap_or(("", local));
                let declaration_scope = qualified(owner, scope);
                let visible_inside = declaration_scope.is_empty()
                    || origin == declaration_scope
                    || origin
                        .strip_prefix(&declaration_scope)
                        .is_some_and(|s| s.starts_with("::"));
                if let Some(hit) = self.import_symbol(m, local) {
                    return (visible_inside
                        || self
                            .facts(m)
                            .iter()
                            .any(|r| r.form == RefForm::Public && r.name == local))
                    .then_some(hit);
                }
                if visiting.len() >= 64 || visiting.contains(&(m, local.into())) {
                    return None;
                }
                visiting.push((m, local.to_owned()));
                let alias_kind = if visible_inside {
                    EdgeKind::Imports
                } else {
                    EdgeKind::Reexports
                };
                let mut aliases = self.facts(m).iter().filter(|r| {
                    r.kind == alias_kind
                        && r.form == RefForm::Ordinary
                        && r.scope == scope
                        && r.alias.as_deref().or_else(|| r.name.rsplit("::").next()) == Some(name)
                });
                let alias = aliases.next()?;
                if aliases.next().is_some() {
                    return None;
                }
                return self.rust_use(root, m, owner, alias, visiting);
            }
            if owner.is_empty() {
                return None;
            }
        }
    }
}
