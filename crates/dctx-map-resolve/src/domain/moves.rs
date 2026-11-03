// SPDX-License-Identifier: Apache-2.0
//! Fingerprint-based move and rename detection between complete snapshots.
//! Implements SPEC-MAP-MODE section 5.2 using parser-normalized body hashes.
//! Uses model nodes and moved-answer statuses; callers persist the revision.
use dctx_map_model::{IdentityStatus, Node, NodeKind, StableId, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};

/// A unique disappeared-to-added identity match at a supplied revision.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Move {
    /// Disappeared node identity.
    pub old: StableId,
    /// Newly introduced identity with the same kind and body fingerprint.
    pub new: StableId,
    /// Caller-supplied revision; the resolver never reads Git or the clock.
    pub rev: String,
}

impl Move {
    /// Model answer notice; the query layer serves `new` after this notice.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn identity(&self) -> IdentityStatus {
        IdentityStatus::Moved {
            old: self.old.clone(),
            new: self.new.clone(),
        }
    }
}

type Fingerprints<'a> = BTreeMap<(NodeKind, Option<SymbolKind>, &'a str), BTreeSet<&'a StableId>>;

/// Detects moves in complete before/after snapshots, sorted by old identity.
/// Only disappeared and newly introduced nodes participate. Empty hashes,
/// changed kinds and ambiguous many-to-one or one-to-many matches are refused.
/// Fingerprints must already be SHA-256 of whitespace-normalized bodies (§5.2).
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn detect_moves(previous: &[Node], current: &[Node], revision: &str) -> Vec<Move> {
    let old_ids: BTreeSet<_> = previous.iter().map(|node| &node.id).collect();
    let new_ids: BTreeSet<_> = current.iter().map(|node| &node.id).collect();
    let removed = fingerprints(previous, &new_ids);
    let added = fingerprints(current, &old_ids);
    let mut moves = Vec::new();
    for (fingerprint, old) in removed {
        let Some(new) = added.get(&fingerprint) else {
            continue;
        };
        if old.len() != 1 || new.len() != 1 {
            continue;
        }
        if let (Some(old), Some(new)) = (old.first(), new.first()) {
            moves.push(Move {
                old: (*old).clone(),
                new: (*new).clone(),
                rev: revision.into(),
            });
        }
    }
    moves.sort();
    moves
}

fn fingerprints<'a>(nodes: &'a [Node], retained: &BTreeSet<&StableId>) -> Fingerprints<'a> {
    let mut groups: Fingerprints<'a> = BTreeMap::new();
    for node in nodes {
        if !node.fingerprint.is_empty() && !retained.contains(&node.id) {
            groups
                .entry((node.kind, node.symbol_kind, &node.fingerprint))
                .or_default()
                .insert(&node.id);
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::fixtures::definition;
    use proptest::prelude::*;

    fn renamed() -> (Node, Node) {
        let mut old = definition(
            "src/old.rs",
            "calculate",
            "old::calculate",
            NodeKind::Symbol,
        )
        .node;
        old.fingerprint = "7fcb0705f9874290b2c0652b984d22f17d59b3ff94e49b617efd69369f12901e".into();
        let mut new = definition("src/new.rs", "compute", "new::compute", NodeKind::Symbol).node;
        new.fingerprint = old.fingerprint.clone();
        (old, new)
    }

    #[test]
    fn rename_fixture_yields_the_model_moved_answer_and_revision() {
        let (old, new) = renamed();
        let moves = detect_moves(
            std::slice::from_ref(&old),
            std::slice::from_ref(&new),
            "abc123",
        );
        assert_eq!(
            moves,
            vec![Move {
                old: old.id.clone(),
                new: new.id.clone(),
                rev: "abc123".into()
            }]
        );
        assert_eq!(
            moves[0].identity(),
            IdentityStatus::Moved {
                old: old.id,
                new: new.id
            }
        );
    }

    #[test]
    fn copies_edits_empty_hashes_and_different_kinds_are_not_moves() {
        let (old, new) = renamed();
        assert_eq!(
            detect_moves(std::slice::from_ref(&old), &[old.clone(), new.clone()], "r"), [] as [crate::domain::moves::Move; 0]
        );
        assert_eq!(
            detect_moves(&[old.clone(), new.clone()], std::slice::from_ref(&new), "r"), [] as [crate::domain::moves::Move; 0]
        );
        let mut changed = new.clone();
        changed.fingerprint.push('0');
        assert_eq!(detect_moves(std::slice::from_ref(&old), &[changed], "r"), [] as [crate::domain::moves::Move; 0]);
        let mut changed = new;
        changed.symbol_kind = Some(SymbolKind::Method);
        assert_eq!(
            detect_moves(
                std::slice::from_ref(&old),
                std::slice::from_ref(&changed),
                "r"
            ), [] as [crate::domain::moves::Move; 0]
        );
        let mut old = old;
        old.fingerprint.clear();
        changed.fingerprint.clear();
        changed.symbol_kind = old.symbol_kind;
        assert_eq!(detect_moves(&[old], &[changed], "r"), [] as [crate::domain::moves::Move; 0]);
    }

    #[test]
    fn identical_bodies_do_not_choose_an_arbitrary_rename() {
        let (old, new) = renamed();
        let mut another = new.clone();
        another.id = "s:src/another.rs#compute".parse().unwrap();
        assert_eq!(
            detect_moves(
                std::slice::from_ref(&old),
                &[new.clone(), another.clone()],
                "r"
            ), [] as [crate::domain::moves::Move; 0]
        );
        assert_eq!(detect_moves(&[old, another], &[new], "r"), [] as [crate::domain::moves::Move; 0]);
    }

    proptest! {
        #[test]
        fn move_detection_is_independent_of_snapshot_order(reverse_old in any::<bool>(), reverse_new in any::<bool>()) {
            let (old, new) = renamed();
            let mut other_old = definition("src/a.rs", "a", "a", NodeKind::Symbol).node;
            let mut other_new = definition("src/b.rs", "b", "b", NodeKind::Symbol).node;
            other_old.fingerprint = "another-body".into();
            other_new.fingerprint = other_old.fingerprint.clone();
            let mut before = vec![old, other_old];
            let mut after = vec![new, other_new];
            let expected = detect_moves(&before, &after, "r");
            prop_assert_eq!(expected.len(), 2);
            if reverse_old { before.reverse(); }
            if reverse_new { after.reverse(); }
            prop_assert_eq!(detect_moves(&before, &after, "r"), expected);
        }
    }
}
