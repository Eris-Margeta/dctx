// SPDX-License-Identifier: Apache-2.0
//! PP10 order and truth: canonical acyclic order (property test), required-predecessor suppression,
//! publication never rolled back, and reports equal host-ledger receipts rather than plugin claims.
//! Traces to Hooks.tla `Plan`, `RunHook`, `Publish`, `Report` with receipts from the real broker.

mod common;

use std::collections::BTreeSet;

use common::*;
use dctx_plugin_host::{HookEntry, HookRun, HookStatus, ModelAction};
use dctx_plugin_protocol::records::scalars::{PluginId, Token};
use proptest::prelude::*;

fn id(n: usize) -> PluginId {
    PluginId::new(&format!("x/p{n}")).expect("id")
}

proptest! {
    /// Every acyclic edge set yields a legal order independent of entry order; cycles are refused.
    #[test]
    fn plan_is_legal_deterministic_or_refused(n in 1usize..6, raw in proptest::collection::vec((0usize..6, 0usize..6), 0..10)) {
        let edges: Vec<(PluginId, PluginId)> = raw.iter().filter(|(a, b)| *a < n && *b < n).map(|(a, b)| (id(*a), id(*b))).collect();
        let entries: Vec<HookEntry> = (0..n).map(|i| HookEntry { plugin: id(i), required: i % 2 == 0 }).collect();
        let mut reversed = entries.clone();
        reversed.reverse();
        match (HookRun::plan(&entries, &edges), HookRun::plan(&reversed, &edges)) {
            (Ok(a), Ok(b)) => {
                prop_assert_eq!(a.order(), b.order());
                let position = |p: &PluginId| a.order().iter().position(|x| x == p);
                for (before, after) in &edges {
                    prop_assert!(position(before) < position(after));
                }
                let unique: BTreeSet<&PluginId> = a.order().iter().collect();
                prop_assert_eq!(unique.len(), n);
            }
            (Err(a), Err(b)) => {
                prop_assert_eq!(a.reason, "hook-cycle");
                prop_assert_eq!(b.reason, "hook-cycle");
                prop_assert!(edges.iter().any(|(x, y)| x == y) || edges.len() >= 2);
            }
            _ => prop_assert!(false, "planning depends on entry order"),
        }
    }
}

#[test]
fn report_lists_ledger_receipts_and_rejects_forged_claims() {
    let mut broker = broker();
    replace_with(&mut broker, b"abc\nx", "a:1").expect("effect");
    let ledger = broker.ledger().committed(&token("op:1"));
    assert_eq!(ledger.len(), 1);
    let mut run = HookRun::plan(&[HookEntry { plugin: id(0), required: false }], &[]).expect("plan");
    let forged = Token::new("receipt:3").expect("forged");
    run.record(true, ledger.clone(), vec![ledger[0].clone(), forged.clone()]).expect("ran");
    run.published();
    let report = run.report();
    assert_eq!(report.receipts, ledger);
    assert_eq!(report.rejected_claims, vec![forged]);
    let trace = run.trace();
    for (a, b) in [
        (ModelAction::HooksPlan, ModelAction::HooksRunHook),
        (ModelAction::HooksRunHook, ModelAction::HooksPublish),
        (ModelAction::HooksPublish, ModelAction::HooksReport),
    ] {
        assert!(trace.precedes(a, b));
    }
}

#[test]
fn post_boundary_failure_keeps_publication_and_effects() {
    let mut run = HookRun::plan(&[HookEntry { plugin: id(0), required: true }], &[]).expect("plan");
    run.published();
    let receipt = Token::new("receipt:1").expect("receipt");
    assert_eq!(run.record(false, vec![receipt.clone()], vec![]), Ok(HookStatus::RequiredFailure));
    let report = run.report();
    assert!(report.published);
    assert_eq!(report.receipts, vec![receipt]);
}

#[test]
fn skipped_hooks_contribute_no_receipts_and_block_transitively() {
    let edges = [(id(0), id(1)), (id(1), id(2))];
    let entries: Vec<HookEntry> = (0..3).map(|i| HookEntry { plugin: id(i), required: true }).collect();
    let mut run = HookRun::plan(&entries, &edges).expect("plan");
    run.record(false, vec![], vec![]).expect("0");
    let stray = Token::new("receipt:9").expect("stray");
    assert_eq!(run.record(true, vec![stray], vec![]), Ok(HookStatus::Skipped));
    assert_eq!(run.record(true, vec![], vec![]), Ok(HookStatus::Skipped));
    assert_eq!(run.report().receipts, [] as [dctx_plugin_protocol::records::scalars::Token; 0]);
}
