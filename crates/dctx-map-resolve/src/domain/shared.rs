// SPDX-License-Identifier: Apache-2.0
//! Path-independent fact payloads placed into a package layout.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-B5 and IDENTITY-v1 sharing.
//! Depends on fact keys and model nodes. The view resolver materializes these.
use super::{FactKey, ResolveError, Target, fact_key::portable_path, facts::canonical};
use dctx_map_model::NodeKind;
use std::collections::BTreeSet;

/// One symbol extracted from file bytes, without a path or package.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SharedSymbol {
    /// Name relative to the file. Empty only for the file module.
    pub local_name: String,
    /// Graph category minted into the view's stable id.
    pub kind: NodeKind,
    /// Optional parsed category. Calls accept any symbol category.
    pub symbol_kind: Option<dctx_map_model::SymbolKind>,
    /// Display name stored on the node. Not a package path.
    pub display_name: String,
    /// Source signature carried into the view.
    pub signature: String,
    /// Body fingerprint. Distinct from the file content digest.
    pub fingerprint: String,
    /// Visibility flag copied onto the node.
    pub public: bool,
    /// Inclusive one-based lines.
    pub lines: [u32; 2],
}

/// A syntactic reference stored with the shared payload.
/// Targets stay spellings; a view binds them to identities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SharedReference {
    /// `local_name` of the symbol that contains this reference.
    pub source_local: String,
    /// Scope relative to the file module.
    pub scope: String,
    /// Relation requested by the extractor.
    pub kind: dctx_map_model::EdgeKind,
    /// Name spelling. Resolved identities are rejected.
    pub target: Target,
    /// Import or reexport binding. Absent uses the last segment.
    pub binding: Option<String>,
}

/// Retained extractor output for one [`FactKey`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFact {
    /// Content and extraction identity.
    pub key: FactKey,
    /// Local symbols. Admission sorts them.
    pub symbols: Vec<SharedSymbol>,
    /// Local references. Admission sorts them.
    pub references: Vec<SharedReference>,
}

impl SharedFact {
    /// Sorts and checks a payload. Order of the inputs is not significant.
    ///
    /// # Errors
    /// Refuses invalid keys, duplicate or malformed symbols, missing reference sources, invalid scopes/bindings and layout-dependent identity targets.
    pub fn new(
        key: FactKey,
        symbols: Vec<SharedSymbol>,
        references: Vec<SharedReference>,
    ) -> Result<Self, ResolveError> {
        let mut fact = Self {
            key,
            symbols,
            references,
        };
        fact.check()?;
        Ok(fact)
    }

    /// Canonicalizes order and rejects layout-dependent or ill-formed payloads.
    ///
    /// # Errors
    /// Refuses invalid keys, duplicate or malformed symbols, missing reference sources, invalid scopes/bindings and layout-dependent identity targets.
    pub fn check(&mut self) -> Result<(), ResolveError> {
        self.key.check()?;
        self.symbols.sort();
        self.references.sort();
        let mut locals = BTreeSet::new();
        let mut modules = 0;
        for symbol in &self.symbols {
            if !locals.insert(symbol.local_name.clone()) {
                return Err(ResolveError::DuplicateSymbol(symbol.local_name.clone()));
            }
            if symbol.lines[0] == 0 || symbol.lines[1] < symbol.lines[0] {
                return Err(ResolveError::InvalidScope(symbol.local_name.clone()));
            }
            if symbol.display_name.is_empty()
                || symbol.display_name.contains(['#', '~', ':', '/', '\\'])
            {
                return Err(ResolveError::InvalidScope(symbol.display_name.clone()));
            }
            match symbol.kind {
                NodeKind::Module => {
                    modules += 1;
                    if !symbol.local_name.is_empty() {
                        return Err(ResolveError::InvalidScope(symbol.local_name.clone()));
                    }
                }
                NodeKind::Symbol | NodeKind::Test | NodeKind::Doc => {
                    if symbol.local_name.is_empty() || !canonical(&symbol.local_name) {
                        return Err(ResolveError::InvalidScope(symbol.local_name.clone()));
                    }
                }
                NodeKind::Place | NodeKind::Package => {
                    return Err(ResolveError::InvalidScope(symbol.display_name.clone()));
                }
            }
        }
        if modules > 1 {
            return Err(ResolveError::DuplicateSymbol("module".into()));
        }
        for reference in &self.references {
            if !locals.contains(&reference.source_local) {
                return Err(ResolveError::MissingSymbol(reference.source_local.clone()));
            }
            if !canonical(&reference.scope) {
                return Err(ResolveError::InvalidScope(reference.scope.clone()));
            }
            match &reference.target {
                Target::Id(_) => return Err(ResolveError::LayoutDependentTarget),
                Target::Name(name) if name.is_empty() => {
                    return Err(ResolveError::InvalidScope(name.clone()));
                }
                Target::Name(_) => {}
            }
            if let Some(binding) = &reference.binding {
                let glob = matches!(&reference.target, Target::Name(name) if name.ends_with("::*"));
                if glob
                    || !matches!(
                        reference.kind,
                        dctx_map_model::EdgeKind::Imports | dctx_map_model::EdgeKind::Reexports
                    )
                    || binding.is_empty()
                    || binding.contains("::")
                    || !canonical(binding)
                {
                    return Err(ResolveError::InvalidBinding(binding.clone()));
                }
            }
        }
        Ok(())
    }
}

/// Where one retained fact sits in a package layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// Place-relative portable path. A `.git` segment is refused.
    pub path: String,
    /// Package identity for this layout.
    pub package: String,
    /// Package-relative module path. Empty at the crate root.
    pub module: String,
    /// Retained fact to place.
    pub key: FactKey,
}

impl Placement {
    pub(crate) fn check(&self) -> Result<(), ResolveError> {
        if !portable_path(&self.path) {
            return Err(ResolveError::InvalidPath(self.path.clone()));
        }
        if self.package.is_empty() || !canonical(&self.package) {
            return Err(ResolveError::InvalidScope(self.package.clone()));
        }
        if !canonical(&self.module) {
            return Err(ResolveError::InvalidScope(self.module.clone()));
        }
        self.key.check()
    }
}

pub(crate) fn join(scope: &str, name: &str) -> String {
    match (scope.is_empty(), name.is_empty()) {
        (true, _) => name.to_owned(),
        (_, true) => scope.to_owned(),
        _ => format!("{scope}::{name}"),
    }
}
