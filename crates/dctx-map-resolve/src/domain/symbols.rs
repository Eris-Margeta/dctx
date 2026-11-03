// SPDX-License-Identifier: Apache-2.0
//! Borrowed symbol table and validation of per-file facts.
//! Implements SPEC-MAP-MODE section 6.3 with package-isolated indexes.
//! Uses parser-neutral definitions; lookup and resolution share this table.
use super::{Definition, Facts, Reference, ResolveError, Target, facts::canonical};
use dctx_map_model::{EdgeKind, Node, NodeKind, StableId};
use std::collections::{BTreeMap, BTreeSet};

type Names<'a> = BTreeMap<(&'a str, &'a str), BTreeSet<StableId>>;

pub(super) struct Entry<'a> {
    pub definition: &'a Definition,
    pub package: &'a str,
}

/// Immutable symbol table borrowing a complete per-file facts snapshot.
/// Names are scoped to a package; explicit identities may cross packages.
pub struct SymbolTable<'a> {
    pub(super) nodes: BTreeMap<StableId, Entry<'a>>,
    pub(super) qualified: Names<'a>,
    pub(super) bare: Names<'a>,
    pub(super) paths: BTreeMap<&'a str, BTreeSet<StableId>>,
    pub(super) imports: BTreeMap<(&'a str, &'a str), Vec<&'a Reference>>,
}

impl<'a> SymbolTable<'a> {
    /// Validates ownership and builds indexes without accessing source files.
    /// Duplicate canonical names remain candidates, never an arbitrary winner.
    ///
    /// # Errors
    /// Refuses duplicate files or nodes, invalid ownership/names/bindings, unknown or foreign sources and unproven containment.
    pub fn new(files: &'a [Facts]) -> Result<Self, ResolveError> {
        let mut table = Self {
            nodes: BTreeMap::new(),
            qualified: BTreeMap::new(),
            bare: BTreeMap::new(),
            paths: BTreeMap::new(),
            imports: BTreeMap::new(),
        };
        let mut paths = BTreeSet::new();
        let mut ordered: Vec<_> = files.iter().collect();
        ordered.sort_by_key(|file| &file.path);
        for file in &ordered {
            if !paths.insert(&file.path) {
                return Err(ResolveError::DuplicateFile(file.path.clone()));
            }
            for definition in &file.definitions {
                table.insert(file, definition)?;
            }
        }
        for file in ordered {
            for reference in &file.references {
                table.validate(file, reference)?;
                if matches!(reference.kind, EdgeKind::Imports | EdgeKind::Reexports) {
                    table
                        .imports
                        .entry((&file.package, &reference.scope))
                        .or_default()
                        .push(reference);
                }
            }
        }
        for imports in table.imports.values_mut() {
            imports.sort();
            imports.dedup();
        }
        Ok(table)
    }

    /// Node associated with an exact stable identity.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn node(&self, id: &StableId) -> Option<&'a Node> {
        self.nodes.get(id).map(|entry| &entry.definition.node)
    }

    /// Number of distinct graph nodes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the snapshot contains no graph nodes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn insert(&mut self, file: &'a Facts, definition: &'a Definition) -> Result<(), ResolveError> {
        let node = &definition.node;
        let identity_path = node
            .id
            .as_str()
            .split_once(':')
            .map(|(_, rest)| rest.split_once('#').map_or(rest, |(path, _)| path));
        if node.path != file.path
            || identity_path != Some(file.path.as_str())
            || !node.id.as_str().starts_with(node.kind.letter())
        {
            return Err(ResolveError::InvalidNode(node.id.clone()));
        }
        if !canonical(&definition.qualified_name) {
            return Err(ResolveError::InvalidScope(
                definition.qualified_name.clone(),
            ));
        }
        if self.nodes.contains_key(&node.id) {
            return Err(ResolveError::DuplicateNode(node.id.clone()));
        }
        self.nodes.insert(
            node.id.clone(),
            Entry {
                definition,
                package: &file.package,
            },
        );
        self.qualified
            .entry((&file.package, &definition.qualified_name))
            .or_default()
            .insert(node.id.clone());
        if let Some(name) = definition
            .qualified_name
            .rsplit("::")
            .next()
            .filter(|s| !s.is_empty())
        {
            self.bare
                .entry((&file.package, name))
                .or_default()
                .insert(node.id.clone());
        }
        if node.kind == NodeKind::Module {
            self.paths
                .entry(&file.path)
                .or_default()
                .insert(node.id.clone());
        }
        Ok(())
    }

    fn validate(&self, file: &Facts, reference: &Reference) -> Result<(), ResolveError> {
        let source = self
            .node(&reference.source)
            .ok_or_else(|| ResolveError::UnknownSource(reference.source.clone()))?;
        if source.path != file.path && reference.kind != EdgeKind::Contains {
            return Err(ResolveError::ForeignSource(reference.source.clone()));
        }
        if !canonical(&reference.scope) {
            return Err(ResolveError::InvalidScope(reference.scope.clone()));
        }
        if let Some(binding) = reference.binding.as_ref().filter(|binding| {
            !matches!(reference.kind, EdgeKind::Imports | EdgeKind::Reexports)
                || binding.is_empty()
                || binding.contains("::")
                || !canonical(binding)
                || matches!(&reference.target, Target::Name(name) if name.ends_with("::*"))
        }) {
            return Err(ResolveError::InvalidBinding(binding.clone()));
        }
        if reference.kind == EdgeKind::Contains && !matches!(reference.target, Target::Id(_)) {
            return Err(ResolveError::UnprovenContainment);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::fixtures::{definition, file, reference};

    #[test]
    fn table_exposes_model_nodes_and_keeps_duplicate_names() {
        let a = definition("src/a.rs", "a", "a::run", NodeKind::Symbol);
        let b = definition("src/b.rs", "b", "a::run", NodeKind::Symbol);
        let files = vec![file("src/a.rs", vec![a.clone()]), file("src/b.rs", vec![b])];
        let table = SymbolTable::new(&files).unwrap();
        assert_eq!(table.len(), 2);
        assert!(!table.is_empty());
        assert_eq!(table.node(&a.node.id), Some(&a.node));
        assert_eq!(
            table.qualified.get(&("fixture", "a::run")).unwrap().len(),
            2
        );
        assert!(SymbolTable::new(&[]).unwrap().is_empty());
    }

    #[test]
    fn invalid_snapshot_contracts_are_typed_errors() {
        let node = definition("src/a.rs", "run", "a::run", NodeKind::Symbol);
        let good = file("src/a.rs", vec![node.clone()]);
        assert!(SymbolTable::new(std::slice::from_ref(&good)).is_ok());
        assert!(matches!(
            SymbolTable::new(&[good.clone(), good.clone()]),
            Err(ResolveError::DuplicateFile(_))
        ));
        let mut bad = good.clone();
        bad.definitions.push(node.clone());
        assert!(matches!(
            SymbolTable::new(&[bad]),
            Err(ResolveError::DuplicateNode(_))
        ));
        let mut bad = good.clone();
        bad.definitions[0].node.path = "foreign.rs".into();
        assert!(matches!(
            SymbolTable::new(&[bad]),
            Err(ResolveError::InvalidNode(_))
        ));
        let mut bad = good.clone();
        bad.definitions[0].qualified_name = "crate::run".into();
        assert!(matches!(
            SymbolTable::new(&[bad]),
            Err(ResolveError::InvalidScope(_))
        ));
        let mut bad = good;
        bad.references
            .push(reference(&node, "a", EdgeKind::Calls, "run"));
        bad.references[0].source = "s:absent.rs#run".parse().unwrap();
        assert!(matches!(
            SymbolTable::new(&[bad]),
            Err(ResolveError::UnknownSource(_))
        ));
    }

    #[test]
    fn references_validate_ownership_scopes_bindings_and_containment() {
        let node = definition("a.rs", "run", "run", NodeKind::Symbol);
        let other = definition("b.rs", "other", "other", NodeKind::Symbol);
        let mut a = file("a.rs", vec![node.clone()]);
        let b = file("b.rs", vec![other.clone()]);
        a.references
            .push(reference(&node, "", EdgeKind::Calls, "other"));
        assert!(SymbolTable::new(&[a.clone(), b.clone()]).is_ok());
        a.references[0].source = other.node.id.clone();
        assert!(matches!(
            SymbolTable::new(&[a.clone(), b.clone()]),
            Err(ResolveError::ForeignSource(_))
        ));
        a.references[0].source = node.node.id.clone();
        a.references[0].scope = "super".into();
        assert!(matches!(
            SymbolTable::new(&[a.clone()]),
            Err(ResolveError::InvalidScope(_))
        ));
        a.references[0].scope.clear();
        a.references[0].binding = Some("alias".into());
        assert!(matches!(
            SymbolTable::new(&[a.clone()]),
            Err(ResolveError::InvalidBinding(_))
        ));
        a.references[0].kind = EdgeKind::Imports;
        assert!(SymbolTable::new(&[a.clone(), b.clone()]).is_ok());
        a.references[0].binding = None;
        a.references[0].kind = EdgeKind::Contains;
        assert!(matches!(
            SymbolTable::new(&[a.clone()]),
            Err(ResolveError::UnprovenContainment)
        ));
        a.references[0].target = Target::Id(other.node.id.clone());
        assert!(SymbolTable::new(&[a, b]).is_ok());
    }
}
