// SPDX-License-Identifier: Apache-2.0
//! Scope, import and reexport traversal with explicit ambiguity.
//! Implements SPEC-MAP-MODE section 6.3 without compiler or filesystem access.
//! Uses the borrowed symbol table; resolution assigns edge confidences.
use super::{Reference, SymbolTable, Target};
use dctx_map_model::{EdgeKind, StableId};
use std::collections::{BTreeMap, BTreeSet};

type Visiting<'a> = BTreeSet<(&'a str, &'a Reference)>;

#[derive(Default)]
pub(super) struct Lookup {
    pub candidates: BTreeMap<StableId, bool>,
    pub bound: bool,
    pub scoped: bool,
}

impl Lookup {
    fn extend(&mut self, other: Self, exact: bool) {
        for (id, path) in other.candidates {
            self.candidates
                .entry(id)
                .and_modify(|value| *value |= path && exact)
                .or_insert(path && exact);
        }
    }

    fn ids(ids: Option<&BTreeSet<StableId>>, exact: bool) -> Self {
        let candidates: BTreeMap<_, _> = ids
            .into_iter()
            .flatten()
            .map(|id| (id.clone(), exact))
            .collect();
        Self {
            bound: !candidates.is_empty(),
            candidates,
            scoped: exact,
        }
    }
}

impl<'a> SymbolTable<'a> {
    pub(super) fn lookup(&self, package: &'a str, reference: &Reference) -> Lookup {
        match &reference.target {
            Target::Name(name) if reference.kind == EdgeKind::Documents => {
                let path = Lookup::ids(self.paths.get(name.as_str()), true);
                if path.bound {
                    return path;
                }
            }
            _ => {}
        }
        self.target(package, reference, &mut Visiting::new())
    }

    fn target(
        &self,
        package: &'a str,
        reference: &Reference,
        visiting: &mut Visiting<'a>,
    ) -> Lookup {
        match &reference.target {
            Target::Id(id) => Lookup {
                candidates: self
                    .nodes
                    .contains_key(id)
                    .then(|| (id.clone(), true))
                    .into_iter()
                    .collect(),
                bound: true,
                scoped: true,
            },
            Target::Name(name) => {
                let name = if matches!(reference.kind, EdgeKind::Imports | EdgeKind::Reexports) {
                    name.strip_suffix("::*").unwrap_or(name)
                } else {
                    name
                };
                self.named(package, &reference.scope, name, visiting)
            }
        }
    }

    fn named(
        &self,
        package: &'a str,
        scope: &str,
        name: &str,
        visiting: &mut Visiting<'a>,
    ) -> Lookup {
        if name.split("::").any(str::is_empty) || name.contains('*') {
            return Lookup::default();
        }
        if name == "crate" {
            return self.at_scope(package, "", "", false, visiting);
        }
        if let Some(name) = name.strip_prefix("crate::") {
            return self.at_scope(package, "", name, false, visiting);
        }
        if name == "self" {
            return self.at_scope(package, scope, "", false, visiting);
        }
        if let Some(name) = name.strip_prefix("self::") {
            return self.at_scope(package, scope, name, false, visiting);
        }
        if name == "super" || name.starts_with("super::") {
            let mut base = scope;
            let mut rest = name;
            while rest == "super" || rest.starts_with("super::") {
                if base.is_empty() {
                    return Lookup::default();
                }
                base = parent(base);
                rest = rest.strip_prefix("super::").unwrap_or("");
            }
            return self.at_scope(package, base, rest, false, visiting);
        }
        let mut scope = scope;
        loop {
            let found = self.at_scope(package, scope, name, false, visiting);
            if found.bound || !found.candidates.is_empty() {
                return found;
            }
            if scope.is_empty() {
                break;
            }
            scope = parent(scope);
        }
        if name.contains("::") {
            Lookup::default()
        } else {
            Lookup::ids(self.bare.get(&(package, name)), false)
        }
    }

    fn at_scope(
        &self,
        package: &'a str,
        scope: &str,
        name: &str,
        exported: bool,
        visiting: &mut Visiting<'a>,
    ) -> Lookup {
        let qualified = join(scope, name);
        let mut found = Lookup::ids(self.qualified.get(&(package, qualified.as_str())), true);
        found.scoped = true;
        let mut globs = Lookup::default();
        let (head, tail) = name.split_once("::").unwrap_or((name, ""));
        if !tail.is_empty() {
            let prefix = join(scope, head);
            if let Some(ids) = self.qualified.get(&(package, prefix.as_str())) {
                found.bound = true;
                for id in ids {
                    found.extend(self.suffix(id, tail, visiting), true);
                }
            }
        }
        if let Some(imports) = self.imports.get(&(package, scope)) {
            for import in imports {
                if exported && import.kind != EdgeKind::Reexports {
                    continue;
                }
                let glob = matches!(&import.target, Target::Name(value) if value.ends_with("::*"));
                if glob && found.bound {
                    continue;
                }
                if !glob && self.binding(import) != Some(head) {
                    continue;
                }
                found.bound |= !glob;
                if !visiting.insert((package, import)) {
                    continue;
                }
                let target = self.target(package, import, visiting);
                let suffix = if glob { name } else { tail };
                let destination = if glob { &mut globs } else { &mut found };
                for (id, exact) in target.candidates {
                    if suffix.is_empty() {
                        destination
                            .candidates
                            .entry(id)
                            .and_modify(|value| *value |= exact)
                            .or_insert(exact);
                    } else {
                        destination.extend(self.suffix(&id, suffix, visiting), exact);
                    }
                }
                visiting.remove(&(package, import));
            }
        }
        if !found.bound {
            found.extend(globs, true);
        }
        found
    }

    fn suffix(&self, id: &StableId, tail: &str, visiting: &mut Visiting<'a>) -> Lookup {
        match self.nodes.get(id) {
            Some(entry) => self.at_scope(
                entry.package,
                &entry.definition.qualified_name,
                tail,
                true,
                visiting,
            ),
            None => Lookup::default(),
        }
    }

    fn binding<'b>(&'b self, reference: &'b Reference) -> Option<&'b str> {
        reference
            .binding
            .as_deref()
            .or_else(|| match &reference.target {
                Target::Name(name) => name.rsplit("::").next(),
                Target::Id(id) => self
                    .nodes
                    .get(id)
                    .and_then(|entry| entry.definition.qualified_name.rsplit("::").next()),
            })
    }
}

fn parent(scope: &str) -> &str {
    scope.rsplit_once("::").map_or("", |(parent, _)| parent)
}

fn join(scope: &str, name: &str) -> String {
    match (scope.is_empty(), name.is_empty()) {
        (true, _) => name.into(),
        (_, true) => scope.into(),
        _ => format!("{scope}::{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::fixtures::{definition, file, reference};
    use dctx_map_model::NodeKind;

    #[test]
    fn anchored_paths_parent_scopes_and_missing_qualifiers() {
        let root = definition("lib.rs", "run", "run", NodeKind::Symbol);
        let local = definition("a.rs", "run", "a::run", NodeKind::Symbol);
        let files = vec![
            file("lib.rs", vec![root.clone()]),
            file("a.rs", vec![local.clone()]),
        ];
        let table = SymbolTable::new(&files).unwrap();
        for (name, scope, expected) in [
            ("crate::run", "a", &root),
            ("self::run", "a", &local),
            ("super::run", "a::nested", &local),
            ("run", "a::nested", &local),
        ] {
            let result = table.lookup("fixture", &reference(&local, scope, EdgeKind::Calls, name));
            assert_eq!(
                result.candidates.keys().collect::<Vec<_>>(),
                vec![&expected.node.id]
            );
            assert!(result.scoped);
        }
        for name in [
            "unknown::run",
            "super::super::run",
            "crate::absent",
            "crate::",
            "a::::run",
        ] {
            assert!(
                table
                    .lookup("fixture", &reference(&local, "a", EdgeKind::Calls, name))
                    .candidates
                    .is_empty()
            );
        }
    }

    #[test]
    fn imports_cycles_remain_unresolved_even_with_a_matching_bare_name() {
        let module = definition("a.rs", "a", "a", NodeKind::Module);
        let decoy = definition("b.rs", "x", "b::x", NodeKind::Symbol);
        let mut a = file("a.rs", vec![module.clone()]);
        let mut x = reference(&module, "a", EdgeKind::Imports, "self::y");
        x.binding = Some("x".into());
        let mut y = reference(&module, "a", EdgeKind::Imports, "self::x");
        y.binding = Some("y".into());
        a.references = vec![x.clone(), y];
        let files = vec![a, file("b.rs", vec![decoy])];
        let table = SymbolTable::new(&files).unwrap();
        assert!(table.lookup("fixture", &x).candidates.is_empty());
        assert!(
            table
                .lookup("fixture", &reference(&module, "a", EdgeKind::Calls, "x"))
                .candidates
                .is_empty()
        );
    }
}
