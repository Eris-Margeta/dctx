// SPDX-License-Identifier: Apache-2.0
//! Scope, import and reexport traversal with explicit ambiguity.
//! Implements SPEC-MAP-MODE section 6.3 without compiler or filesystem access.
//! Uses the borrowed symbol table; resolution assigns edge confidences.
use super::{Reference, SymbolTable, Target};
use dctx_map_model::{EdgeKind, StableId};
use std::collections::{BTreeMap, BTreeSet, btree_map, btree_set};

#[derive(Default)]
pub(super) struct Lookup {
    pub candidates: BTreeMap<StableId, bool>,
    pub bound: bool,
    pub scoped: bool,
}

impl Lookup {
    fn extend(&mut self, other: Self, exact: bool) {
        for (id, path) in other.candidates {
            self.insert(id, path && exact);
        }
    }

    fn insert(&mut self, id: StableId, exact: bool) {
        self.candidates
            .entry(id)
            .and_modify(|value| *value |= exact)
            .or_insert(exact);
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
    pub(super) fn lookup(&self, package: &'a str, reference: &'a Reference) -> Lookup {
        if let Target::Name(name) = &reference.target
            && reference.kind == EdgeKind::Documents
        {
            let path = Lookup::ids(self.paths.get(name.as_str()), true);
            if path.bound {
                return path;
            }
        }
        Search {
            table: self,
            visiting: BTreeSet::new(),
            frames: Vec::new(),
        }
        .run(Step::Target(package, reference))
    }
}

enum Step<'a> {
    Target(&'a str, &'a Reference),
    Scope {
        package: &'a str,
        scope: &'a str,
        name: &'a str,
        exported: bool,
    },
    Done(Lookup),
}

// Each frame resumes a suspended lookup after its child has produced a result.
enum Frame<'a, 'table> {
    Parent(&'a str, &'a str, &'a str),
    Prefix(Scope<'a, 'table>),
    ImportTarget(Import<'a, 'table>),
    ImportSuffix(
        Import<'a, 'table>,
        btree_map::IntoIter<StableId, bool>,
        bool,
    ),
}

struct Scope<'a, 'table> {
    package: &'a str,
    name: &'a str,
    tail: &'a str,
    exported: bool,
    glob: bool,
    prefixes: Option<btree_set::IntoIter<StableId>>,
    imports: std::slice::Iter<'table, &'a Reference>,
    found: Lookup,
}

struct Import<'a, 'table> {
    scope: Scope<'a, 'table>,
    reference: &'a Reference,
}

struct Search<'a, 'table> {
    table: &'table SymbolTable<'a>,
    visiting: BTreeSet<(&'a str, &'a Reference)>,
    frames: Vec<Frame<'a, 'table>>,
}

impl<'a, 'table> Search<'a, 'table> {
    fn run(mut self, mut step: Step<'a>) -> Lookup {
        loop {
            step = match step {
                Step::Target(package, reference) => self.target(package, reference),
                Step::Scope {
                    package,
                    scope,
                    name,
                    exported,
                } => self.at_scope(package, scope, name, exported),
                Step::Done(found) => match self.frames.pop() {
                    Some(frame) => self.resume(frame, found),
                    None => return found,
                },
            };
        }
    }

    fn resume(&mut self, frame: Frame<'a, 'table>, found: Lookup) -> Step<'a> {
        match frame {
            Frame::Parent(package, scope, name) => {
                if found.bound || !found.candidates.is_empty() {
                    Step::Done(found)
                } else if !scope.is_empty() {
                    self.lexical_scope(package, parent(scope), name)
                } else if name.contains("::") {
                    Step::Done(Lookup::default())
                } else {
                    Step::Done(Lookup::ids(self.table.bare.get(&(package, name)), false))
                }
            }
            Frame::Prefix(mut scope) => {
                scope.found.extend(found, true);
                self.scan_scope(scope)
            }
            Frame::ImportTarget(import) => {
                self.scan_candidates(import, found.candidates.into_iter())
            }
            Frame::ImportSuffix(mut import, candidates, exact) => {
                import.scope.found.extend(found, exact);
                self.scan_candidates(import, candidates)
            }
        }
    }

    fn target(&mut self, package: &'a str, reference: &'a Reference) -> Step<'a> {
        match &reference.target {
            Target::Id(id) => Step::Done(Lookup {
                candidates: self
                    .table
                    .nodes
                    .contains_key(id)
                    .then(|| (id.clone(), true))
                    .into_iter()
                    .collect(),
                bound: true,
                scoped: true,
            }),
            Target::Name(name) => {
                let name = if matches!(reference.kind, EdgeKind::Imports | EdgeKind::Reexports) {
                    name.strip_suffix("::*").unwrap_or(name)
                } else {
                    name
                };
                self.named(package, &reference.scope, name)
            }
        }
    }

    fn named(&mut self, package: &'a str, scope: &'a str, name: &'a str) -> Step<'a> {
        if name.split("::").any(str::is_empty) || name.contains('*') {
            return Step::Done(Lookup::default());
        }
        let (scope, name) = if name == "crate" {
            ("", "")
        } else if let Some(name) = name.strip_prefix("crate::") {
            ("", name)
        } else if name == "self" {
            (scope, "")
        } else if let Some(name) = name.strip_prefix("self::") {
            (scope, name)
        } else if name == "super" || name.starts_with("super::") {
            let mut base = scope;
            let mut rest = name;
            while rest == "super" || rest.starts_with("super::") {
                if base.is_empty() {
                    return Step::Done(Lookup::default());
                }
                base = parent(base);
                rest = rest.strip_prefix("super::").unwrap_or("");
            }
            (base, rest)
        } else {
            return self.lexical_scope(package, scope, name);
        };
        Step::Scope {
            package,
            scope,
            name,
            exported: false,
        }
    }

    fn lexical_scope(&mut self, package: &'a str, scope: &'a str, name: &'a str) -> Step<'a> {
        self.frames.push(Frame::Parent(package, scope, name));
        Step::Scope {
            package,
            scope,
            name,
            exported: false,
        }
    }

    fn at_scope(
        &mut self,
        package: &'a str,
        scope: &'a str,
        name: &'a str,
        exported: bool,
    ) -> Step<'a> {
        let qualified = join(scope, name);
        let mut found = Lookup::ids(
            self.table.qualified.get(&(package, qualified.as_str())),
            true,
        );
        found.scoped = true;
        let (head, tail) = name.split_once("::").unwrap_or((name, ""));
        let prefixes = (!tail.is_empty())
            .then(|| join(scope, head))
            .and_then(|prefix| {
                self.table
                    .qualified
                    .get(&(package, prefix.as_str()))
                    .cloned()
            });
        found.bound |= prefixes.is_some();
        let named = self
            .table
            .imports
            .get(&(package, scope, Some(head)))
            .filter(|imports| {
                !exported
                    || imports
                        .iter()
                        .any(|reference| reference.kind == EdgeKind::Reexports)
            });
        // A definition or explicit binding shadows every glob, even if the
        // explicit binding's target cannot resolve.
        found.bound |= named.is_some();
        let glob = !found.bound;
        let imports = if glob {
            self.table.imports.get(&(package, scope, None))
        } else {
            named
        };
        self.scan_scope(Scope {
            package,
            name,
            tail,
            exported,
            glob,
            prefixes: prefixes.map(BTreeSet::into_iter),
            imports: imports.map_or(&[][..], Vec::as_slice).iter(),
            found,
        })
    }

    fn scan_scope(&mut self, mut scope: Scope<'a, 'table>) -> Step<'a> {
        if let Some(id) = scope.prefixes.as_mut().and_then(Iterator::next) {
            let step = self.suffix(&id, scope.tail);
            self.frames.push(Frame::Prefix(scope));
            return step;
        }

        for &reference in scope.imports.by_ref() {
            if scope.exported && reference.kind != EdgeKind::Reexports {
                continue;
            }
            if self.visiting.insert((scope.package, reference)) {
                let step = Step::Target(scope.package, reference);
                self.frames
                    .push(Frame::ImportTarget(Import { scope, reference }));
                return step;
            }
        }
        Step::Done(scope.found)
    }

    fn scan_candidates(
        &mut self,
        mut import: Import<'a, 'table>,
        mut candidates: btree_map::IntoIter<StableId, bool>,
    ) -> Step<'a> {
        let suffix = if import.scope.glob {
            import.scope.name
        } else {
            import.scope.tail
        };
        if suffix.is_empty() {
            for (id, exact) in candidates {
                import.scope.found.insert(id, exact);
            }
        } else if let Some((id, exact)) = candidates.next() {
            let step = self.suffix(&id, suffix);
            self.frames
                .push(Frame::ImportSuffix(import, candidates, exact));
            return step;
        }
        self.visiting
            .remove(&(import.scope.package, import.reference));
        self.scan_scope(import.scope)
    }

    fn suffix(&self, id: &StableId, tail: &'a str) -> Step<'a> {
        match self.table.nodes.get(id) {
            Some(entry) => Step::Scope {
                package: entry.package,
                scope: &entry.definition.qualified_name,
                name: tail,
                exported: true,
            },
            None => Step::Done(Lookup::default()),
        }
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
