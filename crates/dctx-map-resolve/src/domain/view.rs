// SPDX-License-Identifier: Apache-2.0
//! Contextual views over retained, content-keyed facts.
//! Implements DX-B5, IDENTITY-v1 views and properties P6 and P7.
//! Depends on materialization, resolution and the reverse index.
//! Callers inject the clock and ranking revision; this module reads neither.
use super::{
    AliasCollision, ExtractionContract, FactKey, NameCollision, Placement, Resolution, ResolveError,
    ReverseIndex, SharedFact, alias_collisions, fact_key::identity_token, materialize, name_collisions,
    resolve, reverse_index,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// One root's package layout. Paths outside an authorization are dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// Opaque root id. A git object id or `.git` path is refused.
    pub root: String,
    /// Trust namespace. Sharing does not cross this boundary by itself.
    pub trust_namespace: String,
    /// Resolver revision recorded in the deterministic view bytes.
    pub resolver_revision: String,
    /// Ranking revision supplied by the caller. This crate does not rank.
    pub ranking_revision: String,
    /// Frozen clock instant supplied by the caller, in their unit.
    pub frozen_clock: u64,
    /// Files placed in this layout. Order does not change the result.
    pub placements: Vec<Placement>,
}

impl View {
    /// Rejects git-shaped identities and empty revisions.
    ///
    /// # Errors
    /// Refuses empty, non-token, path-shaped or git-shaped root, namespace or revision identities.
    pub fn validate(&self) -> Result<(), ResolveError> {
        for value in [
            &self.root,
            &self.trust_namespace,
            &self.resolver_revision,
            &self.ranking_revision,
        ] {
            if !identity_token(value) || value.contains(['/', '\\']) {
                return Err(ResolveError::InvalidRoot(value.clone()));
            }
        }
        Ok(())
    }

    /// Keeps placements whose path is authorized. Omitted paths are not served.
    /// An empty set authorizes nothing.
    #[allow(clippy::must_use_candidate, clippy::return_self_not_must_use, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn authorize(&self, included: &BTreeSet<String>) -> Self {
        let mut view = self.clone();
        view.placements
            .retain(|placement| included.contains(&placement.path));
        view
    }
}

/// Resolved edges for one view, plus collisions and the reverse index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewResolution {
    /// Root whose layout was resolved.
    pub root: String,
    /// Trust namespace copied from the view.
    pub trust_namespace: String,
    /// Resolver revision copied from the view.
    pub resolver_revision: String,
    /// Ranking revision copied from the view.
    pub ranking_revision: String,
    /// Frozen clock copied from the view.
    pub frozen_clock: u64,
    /// Edges and unresolved references for this layout.
    pub resolution: Resolution,
    /// Import aliases that compete. Never reduced to one target.
    pub alias_collisions: Vec<AliasCollision>,
    /// Display names that belong to several nodes.
    pub name_collisions: Vec<NameCollision>,
    /// Reverse dependents of this view's exports and spellings.
    pub reverse: ReverseIndex,
}

impl ViewResolution {
    /// Canonical UTF-8 result. Equal inputs produce equal bytes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    #[allow(clippy::expect_used, reason = "writing to a String cannot fail")]
    pub fn canonical_bytes(&self) -> String {
        let mut out = String::new();
        writeln!(out, "root\t{}\nns\t{}\nresolver\t{}\nranking\t{}\nclock\t{}",
            self.root,
            self.trust_namespace,
            self.resolver_revision,
            self.ranking_revision,
            self.frozen_clock
        ).expect("writing to a String cannot fail");
        for collision in &self.alias_collisions {
            writeln!(out, "alias\t{}\t{}\t{}\t{}",
                collision.package,
                collision.scope,
                collision.binding,
                collision.targets.join(",")
            ).expect("writing to a String cannot fail");
        }
        for collision in &self.name_collisions {
            let ids = collision
                .candidates
                .iter()
                .map(dctx_map_model::StableId::as_str)
                .collect::<Vec<_>>()
                .join(",");
            writeln!(out, "name\t{}\t{}\t{ids}",
                collision.package, collision.name
            ).expect("writing to a String cannot fail");
        }
        for edge in &self.resolution.edges {
            writeln!(out, "edge\t{}\t{}\t{}\t{}\t{}",
                edge.owner_path,
                edge.src,
                edge.kind.as_str(),
                edge.dst,
                edge.conf.as_str()
            ).expect("writing to a String cannot fail");
        }
        for item in &self.resolution.unresolved {
            write!(out,"open\t{}\t", item.owner_path).expect("writing to a String cannot fail");
            match &item.reason {
                super::UnresolvedReason::Missing => out.push_str("missing"),
                super::UnresolvedReason::Ambiguous(ids) => {
                    out.push_str("ambiguous");
                    for id in ids {
                        out.push('\t');
                        out.push_str(id.as_str());
                    }
                }
            }
            out.push('\n');
        }
        for ((package, name), paths) in &self.reverse.dependents {
            for path in paths {
                writeln!(out, "dep\t{package}\t{name}\t{path}").expect("writing to a String cannot fail");
            }
        }
        for ((package, name), paths) in &self.reverse.users {
            for path in paths {
                writeln!(out, "use\t{package}\t{name}\t{path}").expect("writing to a String cannot fail");
            }
        }
        out
    }
}

/// Whether [`FactStore::admit`] parsed a new key or reused a retained one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The key was absent. It is retained and the parse counter advanced.
    Parsed,
    /// The retained symbols and references already match.
    Reused,
}

/// Retained fact payloads. One slot per [`FactKey`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactStore {
    retained: BTreeMap<FactKey, SharedFact>,
    parses: u64,
}

impl Default for FactStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FactStore {
    /// Empty store. Nothing has been parsed.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn new() -> Self {
        Self {
            retained: BTreeMap::new(),
            parses: 0,
        }
    }

    /// Number of payloads inserted. Reuse and conflicts do not increment it.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn parses(&self) -> u64 {
        self.parses
    }

    /// Number of keys currently retained.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn retained_count(&self) -> usize {
        self.retained.len()
    }

    /// Whether this key still has a payload.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn contains(&self, key: &FactKey) -> bool {
        self.retained.contains_key(key)
    }

    /// Retains a payload, or reuses it when the key and payload already match.
    ///
    /// # Errors
    /// Refuses invalid shared facts or a key that already retains different symbols or references.
    pub fn admit(&mut self, mut fact: SharedFact) -> Result<Admission, ResolveError> {
        fact.check()?;
        if let Some(existing) = self.retained.get(&fact.key) {
            if existing.symbols == fact.symbols && existing.references == fact.references {
                return Ok(Admission::Reused);
            }
            return Err(ResolveError::ConflictingFact);
        }
        self.parses += 1;
        self.retained.insert(fact.key.clone(), fact);
        Ok(Admission::Parsed)
    }

    /// Drops a retained key, as garbage collection would. The next admit reparses.
    pub fn reclaim(&mut self, key: &FactKey) -> bool {
        self.retained.remove(key).is_some()
    }

    /// Materializes placement facts without resolving them.
    ///
    /// # Errors
    /// Refuses invalid view identities or placements, missing retained facts/symbols and invalid materialized identities.
    pub fn placed_facts(&self, view: &View) -> Result<Vec<super::Facts>, ResolveError> {
        view.validate()?;
        materialize(&self.retained, &view.placements)
    }

    /// Resolves one authorized layout from retained facts.
    ///
    /// # Errors
    /// Propagates view validation, materialization and resolver input-contract errors.
    pub fn resolve_view(&self, view: &View) -> Result<ViewResolution, ResolveError> {
        view.validate()?;
        let files = materialize(&self.retained, &view.placements)?;
        let resolution = resolve(&files)?;
        Ok(ViewResolution {
            root: view.root.clone(),
            trust_namespace: view.trust_namespace.clone(),
            resolver_revision: view.resolver_revision.clone(),
            ranking_revision: view.ranking_revision.clone(),
            frozen_clock: view.frozen_clock,
            alias_collisions: alias_collisions(&files),
            name_collisions: name_collisions(&files),
            reverse: reverse_index(&files, &resolution),
            resolution,
        })
    }
}

/// Contract helper used by tests and callers that share one extractor revision.
///
/// # Errors
/// Refuses empty, non-token or git-shaped revision and plan labels.
pub fn contract(revision: &str, plan: &str) -> Result<ExtractionContract, ResolveError> {
    let contract = ExtractionContract {
        language: 1,
        dialect: "rust-2024".into(),
        parser_id: "dctx-map-parse".into(),
        parser_revision: revision.into(),
        extraction_digest: plan.into(),
        fact_schema: 1,
    };
    contract.check()?;
    Ok(contract)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FactKey, Placement, ResolveError};
    use dctx_map_model::ContentId;

    #[test]
    fn git_shaped_roots_are_refused_and_opaque_roots_resolve_when_retained() {
        let mut rejected = View {
            root: "da39a3ee5e6b4b0d3255bfef95601890afd80709".into(),
            trust_namespace: "local".into(),
            resolver_revision: "resolve-1".into(),
            ranking_revision: "rank-1".into(),
            frozen_clock: 1,
            placements: vec![],
        };
        assert!(matches!(
            rejected.validate(),
            Err(ResolveError::InvalidRoot(_))
        ));
        rejected.root = "root-app".into();
        assert!(rejected.validate().is_ok());
        rejected.root = "src/.git".into();
        assert!(rejected.validate().is_err());
        rejected.root = "root-app".into();
        let key = FactKey::new(ContentId::of_bytes(b"fn work() {}").unwrap(), contract("p1", "plan").unwrap()).unwrap();
        let store = FactStore::new();
        assert!(matches!(
            store.resolve_view(&View {
                placements: vec![Placement {
                    path: "src/lib.rs".into(),
                    package: "app".into(),
                    module: String::new(),
                    key: key.clone(),
                }],
                ..rejected
            }),
            Err(ResolveError::MissingFact(_))
        ));
        assert_eq!(store.parses(), 0);
    }
}
