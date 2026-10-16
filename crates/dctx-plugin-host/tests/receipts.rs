//! PP3 effect audit and PP5 replay through the broker, plus a ledger state-machine property test.
//! Traces to Receipts.tla `Request`, `Execute`, `Complete`, `CrashOrUncertain`, `AuditFailure`, `Repeat`,
//! `Reconcile`, `Rotate` and Authority.tla `Admin` for the git recipe. Every refusal has a control.

mod common;

use common::*;
use dctx_plugin_host::fakes::FakeAudit;
use dctx_plugin_host::ports::{AuditKind, RecipeOutcome};
use dctx_plugin_host::{ArgDigest, Decision, EntryStatus, HandleKind, Ledger, Lookup, ModelAction, Reply, Selection};
use dctx_plugin_protocol::records::scalars::Token;
use dctx_plugin_protocol::{EffectState, ErrorCode};
use proptest::prelude::*;

fn receipt(reply: &Reply) -> Token {
    match reply {
        Reply::Receipt { receipt, .. } => receipt.clone(),
        other => panic!("expected receipt: {other:?}"),
    }
}

#[test]
fn intent_precedes_effect_and_completion_control() {
    let mut broker = broker();
    let reply = replace_with(&mut broker, b"abc\nx", "a:1").expect("committed");
    let kinds: Vec<AuditKind> = broker
        .audit()
        .records
        .iter()
        .filter(|r| r.method == "file.replace")
        .map(|r| r.kind)
        .collect();
    assert_eq!(kinds, vec![AuditKind::Intent, AuditKind::Completion]);
    let id = receipt(&reply);
    assert_eq!(broker.ledger().committed(&token("op:1")), vec![id]);
    let trace = broker.ledger().trace();
    assert!(trace.precedes(ModelAction::ReceiptsRequest, ModelAction::ReceiptsExecute));
    assert!(trace.precedes(ModelAction::ReceiptsExecute, ModelAction::ReceiptsComplete));
    assert!(broker.authority().trace().precedes(ModelAction::AuthorityAdmit, ModelAction::AuthorityCommit));
}

#[test]
fn duplicate_key_and_args_returns_receipt_with_zero_new_effects() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let pre = first_read(&mut broker, &handle);
    let upload = stage(&mut broker, &handle, &pre, b"abc\nx");
    let request = replace_req(&handle, &pre, &upload, "a:1");
    let first = receipt(&broker.handle(&request).expect("first"));
    let again = broker.handle(&request).expect("replay");
    assert!(matches!(&again, Reply::Receipt { replayed: true, .. }));
    assert_eq!(receipt(&again), first);
    assert_eq!(broker.files().replaces, 1);
    assert_eq!(broker.audit().count(AuditKind::Intent), 1);
}

#[test]
fn reused_action_id_with_different_args_conflicts() {
    let mut broker = broker();
    replace_with(&mut broker, b"abc\nx", "a:1").expect("first");
    let refused = replace_with(&mut broker, b"abc\ny", "a:1").expect_err("conflict");
    assert_eq!(refused.code, ErrorCode::Conflict);
    assert_eq!(broker.files().replaces, 1);
    assert!(replace_with(&mut broker, b"abc\nxy", "a:2").is_ok());
}

#[test]
fn audit_unavailable_refuses_new_effects_with_restore_control() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let pre = first_read(&mut broker, &handle);
    let upload = stage(&mut broker, &handle, &pre, b"abc\nx");
    broker.audit_mut().available = false;
    let refused = broker.handle(&replace_req(&handle, &pre, &upload, "a:1")).expect_err("audit");
    assert_eq!((refused.code, refused.effect_state), (ErrorCode::AuditUnavailable, EffectState::None));
    assert_eq!(broker.files().replaces, 0);
    assert!(broker.ledger().trace().contains(ModelAction::ReceiptsAuditFailure));
    broker.audit_mut().available = true;
    assert!(broker.handle(&replace_req(&handle, &pre, &upload, "a:1")).is_ok());
    assert_eq!(broker.files().replaces, 1);
}

#[test]
fn audit_unavailable_blocks_delivery() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    broker.audit_mut().fail_next = Some(AuditKind::Delivery);
    let refused = broker.handle(&read_req(&handle, 0, 3, None)).expect_err("no delivery");
    assert_eq!(refused.code, ErrorCode::AuditUnavailable);
    assert!(!broker.authority().trace().contains(ModelAction::AuthorityRead));
    assert!(broker.handle(&read_req(&handle, 0, 3, None)).is_ok());
}

#[test]
fn completion_log_failure_leaves_unknown_pinned_until_reconciled() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let pre = first_read(&mut broker, &handle);
    let upload = stage(&mut broker, &handle, &pre, b"abc\nx");
    let request = replace_req(&handle, &pre, &upload, "a:1");
    broker.audit_mut().fail_next = Some(AuditKind::Completion);
    let unknown = broker.handle(&request).expect_err("unknown");
    assert_eq!((unknown.code, unknown.effect_state), (ErrorCode::EffectUnknown, EffectState::Unknown));
    assert!(unknown.receipt.is_some() && !unknown.retryable());
    assert_eq!(broker.files().replaces, 1);
    let replay = broker.handle(&request).expect_err("no blind retry");
    assert_eq!(replay.code, ErrorCode::EffectUnknown);
    assert_eq!(broker.files().replaces, 1);
    let (key, _) = broker.replay_key(&request).expect("key");
    assert!(broker.rotate(&key).is_err());
    assert!(broker.reconcile(&key, false).is_err());
    broker.reconcile(&key, true).expect("reconciled");
    assert_eq!(broker.ledger().get(&key).map(|e| e.status), Some(EntryStatus::Committed));
    assert!(matches!(broker.handle(&request), Ok(Reply::Receipt { replayed: true, .. })));
    broker.rotate(&key).expect("rotated");
    assert!(matches!(broker.handle(&request), Ok(Reply::Receipt { replayed: true, .. })));
    assert_eq!(broker.files().replaces, 1);
}

#[test]
fn uncertain_port_outcome_is_unknown_not_success() {
    let mut broker = broker();
    broker.files_mut().uncertain_next_replace = true;
    let unknown = replace_with(&mut broker, b"abc\nx", "a:1").expect_err("uncertain");
    assert_eq!(unknown.effect_state, EffectState::Unknown);
    assert!(broker.ledger().trace().contains(ModelAction::ReceiptsCrashOrUncertain));
    assert!(broker.ledger().committed(&token("op:1")).is_empty());
}

#[test]
fn git_recipe_commits_with_admin_trace_and_refuses_excluded_ignore_file() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let repo = broker.register(HandleKind::Repository).expect("repo");
    let pre = first_read(&mut broker, &handle);
    let reply = broker.handle(&git_req(&handle, &pre, &repo, "g:1")).expect("staged");
    assert!(matches!(reply, Reply::Receipt { replayed: false, .. }));
    assert_eq!(broker.recipes().recipes.len(), 1);
    assert!(broker.authority().trace().contains(ModelAction::AuthorityAdmin));

    broker.authority_mut().change_selection(Selection::new([(path(), Decision::OmitContent)]));
    let refused = broker.handle(&git_req(&handle, &pre, &repo, "g:2")).expect_err("excluded");
    assert_eq!(refused.code, ErrorCode::Denied);
    assert_eq!(broker.recipes().recipes.len(), 1);
}

#[test]
fn failed_recipe_reports_failure_and_commits_nothing() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let repo = broker.register(HandleKind::Repository).expect("repo");
    let pre = first_read(&mut broker, &handle);
    broker.recipes_mut().recipe = RecipeOutcome::Failed;
    let refused = broker.handle(&git_req(&handle, &pre, &repo, "g:1")).expect_err("failed");
    assert_eq!((refused.code, refused.effect_state), (ErrorCode::ExternalProgramFailed, EffectState::None));
    assert!(broker.ledger().committed(&token("op:1")).is_empty());
    broker.recipes_mut().recipe = RecipeOutcome::Succeeded;
    assert!(broker.handle(&git_req(&handle, &pre, &repo, "g:1")).is_ok());
}

#[test]
fn foreign_repository_handle_is_refused() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let foreign = broker.register_bound(binding("op:9"), HandleKind::Repository).expect("foreign");
    let pre = first_read(&mut broker, &handle);
    assert_eq!(broker.handle(&git_req(&handle, &pre, &foreign, "g:1")).map_err(|e| e.reason), Err("not-the-bound-repository"));
    assert_eq!(broker.recipes().recipes.len(), 0);
}

#[test]
fn audit_full_log_refuses_admission() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let pre = first_read(&mut broker, &handle);
    let upload = stage(&mut broker, &handle, &pre, b"abc\nx");
    let records = broker.audit().records.clone();
    *broker.audit_mut() = FakeAudit { capacity: records.len(), records, ..FakeAudit::default() };
    assert_eq!(broker.handle(&replace_req(&handle, &pre, &upload, "a:1")).map_err(|e| e.code), Err(ErrorCode::AuditUnavailable));
    assert_eq!(broker.files().replaces, 0);
}

#[derive(Clone, Debug)]
enum Op {
    Request(usize, bool),
    Execute(usize),
    Complete(usize),
    Uncertain(usize),
    Reconcile(usize),
    Rotate(usize),
}

fn op() -> impl Strategy<Value = Op> {
    let slot = 0..2usize;
    prop_oneof![
        (slot.clone(), any::<bool>()).prop_map(|(i, a)| Op::Request(i, a)),
        slot.clone().prop_map(Op::Execute),
        slot.clone().prop_map(Op::Complete),
        slot.clone().prop_map(Op::Uncertain),
        slot.clone().prop_map(Op::Reconcile),
        slot.prop_map(Op::Rotate),
    ]
}

proptest! {
    /// PP5: within the ledger each key executes at most once and `Existing` only for equal arguments.
    #[test]
    fn ledger_never_executes_a_key_twice(ops in proptest::collection::vec(op(), 0..40)) {
        let keys = [
            dctx_plugin_host::ActionKey::bind(&binding("op:1"), &token("a:1")),
            dctx_plugin_host::ActionKey::bind(&binding("op:2"), &token("a:1")),
        ];
        let args = [ArgDigest::of(b"a").expect("a"), ArgDigest::of(b"b").expect("b")];
        let mut ledger = Ledger::new(4);
        let mut bound: [Option<ArgDigest>; 2] = [None, None];
        for op in ops {
            match op {
                Op::Request(i, second) => {
                    let arg = args[usize::from(second)];
                    match ledger.lookup(&keys[i], &arg) {
                        Ok(Lookup::Absent) => {
                            let receipt = ledger.reserve().expect("receipt");
                            ledger.intent(keys[i].clone(), arg, receipt).expect("intent");
                            bound[i] = Some(arg);
                        }
                        Ok(Lookup::Existing(_)) => prop_assert_eq!(bound[i], Some(arg)),
                        Err(_) => {}
                    }
                }
                Op::Execute(i) => { let _ = ledger.executed(&keys[i]); }
                Op::Complete(i) => { let _ = ledger.complete(&keys[i]); }
                Op::Uncertain(i) => { let _ = ledger.uncertain(&keys[i], false); }
                Op::Reconcile(i) => { let _ = ledger.reconcile(&keys[i]); }
                Op::Rotate(i) => {
                    let status = ledger.get(&keys[i]).map(|e| e.status);
                    let rotated = ledger.rotate(&keys[i]).is_ok();
                    prop_assert!(!rotated || status == Some(EntryStatus::Committed));
                }
            }
            for key in &keys {
                if let Some(entry) = ledger.get(key) {
                    prop_assert!(entry.executions <= 1);
                    prop_assert!(!entry.archived || entry.status == EntryStatus::Committed);
                }
            }
        }
    }
}
