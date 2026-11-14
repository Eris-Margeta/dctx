// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs for host domains and services. Typed requests already contain
//! validated wire scalars; raw content and digest parsing additionally receive arbitrary bytes.
//! All observation times are explicit integers, and all effects use deterministic fake ports.

mod common;

use common::*;
use dctx_plugin_host::domain::budget::{base64_len, chunk_fits_frame};
use dctx_plugin_host::domain::grant::{covers, intersect, intersect_sets};
use dctx_plugin_host::domain::liveness::MAX_TRACKED;
use dctx_plugin_host::domain::trace::TRACE_CAP;
use dctx_plugin_host::fakes::*;
use dctx_plugin_host::ports::*;
use dctx_plugin_host::service::handles::{Handles, MAX_HANDLES, Upload};
use dctx_plugin_host::*;
use dctx_plugin_protocol::records::broker::FileStage;
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::capability::Liveness;
use dctx_plugin_protocol::records::common::Wait;
use dctx_plugin_protocol::records::events::{Progress, ProgressUnit};
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{PluginId, RequestId};
use dctx_plugin_protocol::{EffectState, ErrorCode};

const POLICY: Liveness = Liveness::UnattendedFailClosed {
    sample_ms: 1,
    cancel_grace_ms: 1,
};

fn key(action: &str) -> ActionKey {
    ActionKey::bind(&binding("op:1"), &token(action))
}
fn args() -> ArgDigest {
    ArgDigest::of(b"arguments").unwrap()
}
fn facts() -> LaunchFacts {
    LaunchFacts {
        origin: Origin::ThirdParty,
        verified: true,
        compatible: true,
        confined: true,
    }
}
fn supervisor() -> Supervisor<FakeProcess> {
    let mut s = Supervisor::new(facts(), POLICY, FakeProcess::default());
    s.launch().unwrap();
    s.negotiated().unwrap();
    s.initialized().unwrap();
    s.invoke(0).unwrap();
    s
}
fn sample(seq: u64, completed: u64) -> Progress {
    Progress {
        invocation: token("op:1"),
        seq,
        phase: dctx_plugin_protocol::records::scalars::Name::new("read").unwrap(),
        completed,
        total: Some(100),
        unit: ProgressUnit::Bytes,
        wait: None,
    }
}

#[test]
fn argument_digest_parsing_truncates_and_repeats_without_panics() {
    let hex = "aB".repeat(32);
    for end in 0..hex.len() {
        assert!(ArgDigest::from_hex(&hex[..end]).is_err());
    }
    assert!(ArgDigest::from_hex(&(hex.clone() + "0")).is_err());
    for bad in ["".to_owned(), "x".into(), "é".repeat(32), "g".repeat(64)] {
        assert!(ArgDigest::from_hex(&bad).is_err());
    }
    assert_eq!(ArgDigest::from_hex(&hex).unwrap().as_bytes(), &[0xab; 32]);
    for bytes in [b"".as_slice(), b"a", b"\xff\0\xc0\x80"] {
        assert_eq!(ArgDigest::of(bytes), ArgDigest::of(bytes));
        for end in 0..=bytes.len() {
            assert!(ArgDigest::of(&bytes[..end]).is_ok());
        }
    }
    let b = broker();
    let request = replace_req(&token("f"), &token("pre"), &token("up"), "a");
    assert_eq!(
        ArgDigest::of_request(b.authority().binding(), &request)
            .unwrap()
            .unwrap(),
        b.replay_key(&request).unwrap()
    );
    assert!(b.replay_key(&read_req(&token("f"), 0, 1, None)).is_err());
}

#[test]
fn quotas_and_arithmetic_cover_zero_max_overflow_and_permutations() {
    assert_eq!(Quota::new(&[]).limit(), 0);
    for ceilings in [&[9, 1, 5][..], &[5, 9, 1], &[1, 1, 9]] {
        let mut q = Quota::new(ceilings);
        assert_eq!(q.limit(), 1);
        q.charge(0).unwrap();
        q.charge(1).unwrap();
        assert!(q.check(1).is_err());
        assert!(q.charge(1).is_err());
        assert_eq!(q.used(), 1);
        q.release(u64::MAX);
        assert_eq!(q.used(), 0);
    }
    let mut q = Quota::new(&[u64::MAX]);
    q.charge(u64::MAX).unwrap();
    assert!(q.charge(1).is_err());
    assert_eq!(q.used(), u64::MAX);
    assert_eq!(base64_len(0), Some(0));
    assert_eq!(base64_len(1), Some(4));
    assert_eq!(base64_len(u64::MAX), None);
    assert!(chunk_fits_frame(3, 1, 5));
    assert!(!chunk_fits_frame(3, 2, 5));
    assert!(!chunk_fits_frame(1, u64::MAX, u64::MAX));
}

#[test]
fn duplicate_dimension_batches_are_atomic_including_integer_overflow() {
    for charges in [
        vec![(Dimension::Content, 6), (Dimension::Content, 5)],
        vec![
            (Dimension::Content, 5),
            (Dimension::Actions, 1),
            (Dimension::Content, 6),
        ],
        vec![
            (Dimension::Actions, 1),
            (Dimension::Content, 6),
            (Dimension::Content, 5),
        ],
    ] {
        let mut b = Budget::new(&[10], &[1], &[10], &[10], &[10]);
        b.charge(&[]).unwrap();
        assert!(b.charge(&charges).is_err());
        for d in [
            Dimension::Content,
            Dimension::Actions,
            Dimension::Upload,
            Dimension::Diagnostics,
            Dimension::Queue,
        ] {
            assert_eq!(b.quota(d).used(), 0);
        }
        b.charge(&[(Dimension::Content, 5), (Dimension::Content, 5)])
            .unwrap();
        assert_eq!(b.quota(Dimension::Content).used(), 10);
        b.release(Dimension::Content, u64::MAX);
        assert_eq!(b.quota(Dimension::Content).used(), 0);
    }
    let mut b = Budget::new(&[u64::MAX], &[], &[], &[], &[]);
    assert!(
        b.charge(&[(Dimension::Content, u64::MAX), (Dimension::Content, 1)])
            .is_err()
    );
    assert_eq!(b.quota(Dimension::Content).used(), 0);
}

#[test]
fn queues_and_traces_are_bounded_and_keep_order_with_duplicates() {
    let mut zero = BoundedQueue::new(0);
    assert!(zero.push(0).is_err());
    assert_eq!(zero.pop(), None);
    let mut queue = BoundedQueue::new(3);
    for n in [2, 1, 2] {
        queue.push(n).unwrap();
    }
    assert!(queue.push(4).is_err());
    assert_eq!(queue.len(), 3);
    assert_eq!(
        [queue.pop(), queue.pop(), queue.pop()],
        [Some(2), Some(1), Some(2)]
    );
    queue.clear();
    queue.clear();
    assert!(queue.is_empty());
    let mut trace = Trace::default();
    for _ in 0..=TRACE_CAP {
        trace.push(ModelAction::AuthorityRead);
    }
    assert_eq!(trace.actions().len(), TRACE_CAP);
    assert_eq!(trace.dropped(), 1);
    assert_eq!(trace.count(ModelAction::AuthorityRead), TRACE_CAP);
    assert!(trace.contains(ModelAction::AuthorityRead));
    assert!(!trace.precedes(ModelAction::HooksPlan, ModelAction::HooksReport));
}

#[test]
fn ledger_direct_intents_cannot_bypass_zero_or_exact_capacity() {
    let mut zero = Ledger::new(0);
    assert!(zero.reserve().is_err());
    assert!(zero.intent(key("a"), args(), token("receipt:1")).is_err());
    assert!(zero.get(&key("a")).is_none());
    let mut ledger = Ledger::new(1);
    ledger.intent(key("a"), args(), token("receipt:1")).unwrap();
    assert!(ledger.intent(key("b"), args(), token("receipt:2")).is_err());
    assert!(ledger.intent(key("a"), args(), token("receipt:3")).is_err());
    assert!(ledger.get(&key("b")).is_none());
    ledger.abandon(&key("a")).unwrap();
    ledger.intent(key("b"), args(), token("receipt:2")).unwrap();
}

#[test]
fn ledger_state_transitions_replay_and_reconcile_are_deterministic() {
    let mut ledger = Ledger::new(2);
    assert_eq!(ledger.lookup(&key("a"), &args()).unwrap(), Lookup::Absent);
    for missing in [key("a"), key("b")] {
        assert!(ledger.executed(&missing).is_err());
        assert!(ledger.complete(&missing).is_err());
        assert!(ledger.uncertain(&missing, false).is_err());
        assert!(ledger.reconcile(&missing).is_err());
        assert!(ledger.rotate(&missing).is_err());
        assert!(ledger.abandon(&missing).is_err());
    }
    let receipt = ledger.reserve().unwrap();
    ledger.intent(key("a"), args(), receipt.clone()).unwrap();
    assert!(ledger.complete(&key("a")).is_err());
    ledger.executed(&key("a")).unwrap();
    assert!(ledger.executed(&key("a")).is_err());
    ledger.uncertain(&key("a"), true).unwrap();
    let refusal = ledger.lookup(&key("a"), &args()).unwrap_err();
    assert!(!refusal.retryable());
    assert_eq!(refusal.effect_state, EffectState::Unknown);
    assert!(ledger.rotate(&key("a")).is_err());
    assert_eq!(ledger.reconcile(&key("a")).unwrap(), receipt);
    for _ in 0..3 {
        assert_eq!(
            ledger.lookup(&key("a"), &args()).unwrap(),
            Lookup::Existing(receipt.clone())
        );
        ledger.rotate(&key("a")).unwrap();
    }
    assert!(
        ledger
            .lookup(&key("a"), &ArgDigest::of(b"different").unwrap())
            .is_err()
    );
    assert_eq!(ledger.get(&key("a")).unwrap().executions, 1);
    assert_eq!(ledger.committed(&token("op:1")), vec![receipt]);
    ledger.audit_failed();
    assert!(ledger.trace().contains(ModelAction::ReceiptsAuditFailure));
}

fn recovery(state: RecoveryState) -> RecoveryRecord {
    RecoveryRecord {
        key: key("a"),
        args: args(),
        receipt: token("receipt:1"),
        state,
    }
}

#[test]
fn recovery_empty_over_max_duplicates_reordering_and_every_record_truncation() {
    Ledger::new(0).restore(&[]).unwrap();
    let records = [
        recovery(RecoveryState::Pending),
        recovery(RecoveryState::Committed),
        recovery(RecoveryState::Archived),
    ];
    for end in 0..=records.len() {
        let mut first = Ledger::new(1);
        first.restore(&records[..end]).unwrap();
        let mut second = Ledger::new(1);
        second.restore(&records[..end]).unwrap();
        assert_eq!(first.get(&key("a")), second.get(&key("a")));
        if end == 1 {
            assert!(first.lookup(&key("a"), &args()).is_err());
        }
        if end > 1 {
            assert!(matches!(
                first.lookup(&key("a"), &args()),
                Ok(Lookup::Existing(_))
            ));
        }
    }
    for bad in [
        vec![records[0].clone(), records[0].clone()],
        vec![records[1].clone(), records[0].clone()],
        vec![records[0].clone(); MAX_RECOVERY_RECORDS + 1],
    ] {
        let mut ledger = Ledger::new(MAX_RECOVERY_RECORDS);
        assert!(ledger.restore(&bad).is_err());
        assert!(ledger.get(&key("a")).is_none());
        assert_eq!(ledger.reserve().unwrap(), token("receipt:1"));
    }
    for bad in ["", "x", "é", "committed\0", "pending "] {
        assert!(RecoveryState::parse(bad).is_err());
    }
    for state in [
        RecoveryState::Pending,
        RecoveryState::Committed,
        RecoveryState::Partial,
        RecoveryState::Unknown,
        RecoveryState::NotRun,
        RecoveryState::Reconciled,
        RecoveryState::Archived,
    ] {
        assert_eq!(RecoveryState::parse(state.as_str()), Ok(state));
    }
}

#[test]
fn authority_checks_foreign_bindings_narrowing_and_epoch_exhaustion() {
    for epoch in [0, u64::MAX - 1, u64::MAX] {
        let mut authority = Authority::new(layers(all_caps()), included(), binding("op:1"), epoch);
        let need = Need::FileRead {
            path: path(),
            bytes: 1,
        };
        assert!(authority.admit(&binding("op:1"), need.clone()).is_err());
        authority.initialize().unwrap();
        assert!(authority.admit(&binding("op:2"), need.clone()).is_err());
        let admitted = authority.admit(&binding("op:1"), need).unwrap();
        authority.commit(&admitted).unwrap();
        authority.approve(Layer::Consent, all_caps());
        assert!(
            authority.commit(&admitted).is_err(),
            "old admission at epoch {epoch}"
        );
        if epoch == u64::MAX {
            assert!(authority.halted());
        }
        authority.halt();
        authority.halt();
        assert!(authority.commit(&admitted).is_err());
        assert!(authority.initialize().is_err());
    }
    let mut authority = Authority::new(layers(all_caps()), included(), binding("op:1"), 0);
    authority.initialize().unwrap();
    authority.configure(&[read_cap()]);
    authority.narrow(Layer::Organization, &[]);
    assert!(authority.effective().is_empty());
    authority.change_selection(Selection::default());
    assert!(authority.check_content(&path()).is_err());
    assert!(authority.check_provenance(&[path()]).is_err());
    assert!(authority.check_provenance(&[]).is_ok());
}

#[test]
fn grants_selection_and_denies_do_not_widen_with_duplicate_items() {
    let selection = Selection::new([(path(), Decision::Include)]);
    assert!(selection.authorizes(&path()));
    assert!(selection.authorizes_all([&path(), &path()]));
    assert_eq!(Selection::default().decision(&path()), Decision::Exclude);
    let a = read_cap();
    let b = write_cap();
    assert_eq!(intersect(&a, &a), Some(a.clone()));
    assert!(intersect(&a, &b).is_none());
    assert_eq!(
        intersect_sets(&[a.clone(), a.clone()], std::slice::from_ref(&a)),
        vec![a.clone()]
    );
    assert!(intersect_sets(&[], std::slice::from_ref(&a)).is_empty());
    assert!(!covers(
        &a,
        &Need::FileRead {
            path: path(),
            bytes: u64::MAX
        }
    ));
    let mut grant = layers(all_caps());
    grant.denies = vec![DenyRule::FileRead(path()), DenyRule::FileRead(path())];
    assert!(!grant.effective(&selection).contains(&a));
    for decision in [
        Decision::Exclude,
        Decision::OmitContent,
        Decision::DebugVisible,
    ] {
        assert!(!decision.authorizes_content());
    }
}

#[test]
fn duplicate_settlement_and_registration_cannot_manufacture_progress_or_resurrect_waits() {
    let dependency = token("dep:1");
    let mut feed = LivenessFeed::new(POLICY, 0);
    feed.register_dependency(dependency.clone()).unwrap();
    feed.settle_dependency(&dependency, true, 0);
    assert_eq!(feed.sample(1, ProcessEvidence::Unknown), Verdict::Continue);
    let advances = feed.trace().count(ModelAction::LifecycleAdvance);
    feed.settle_dependency(&dependency, true, 1);
    assert_eq!(feed.trace().count(ModelAction::LifecycleAdvance), advances);
    feed.register_dependency(dependency.clone()).unwrap();
    assert_eq!(feed.sample(2, ProcessEvidence::Unknown), Verdict::SendProbe);
    assert!(matches!(
        feed.probe_reply(Some(&Wait::Dependency(dependency)), 2),
        Verdict::Cancel(_)
    ));
}

#[test]
fn liveness_progress_bounds_duplicates_regression_and_tracking_capacity() {
    let mut feed = LivenessFeed::new(POLICY, 0);
    assert!(feed.progress(&sample(0, 0), 0).is_err());
    assert!(feed.progress(&sample(1, 101), 0).is_err());
    assert!(feed.progress(&sample(1, 1), 0).unwrap());
    assert!(feed.progress(&sample(1, 1), 0).is_err());
    assert!(feed.progress(&sample(2, 0), 0).is_err());
    assert!(!feed.progress(&sample(2, 1), 0).unwrap());
    assert!(feed.progress(&sample(3, 2), 0).unwrap());
    assert_eq!(feed.recent().count(), 2);
    for i in 0..MAX_TRACKED {
        feed.broker_started(RequestId::new(&format!("p:{i}")).unwrap())
            .unwrap();
        feed.register_dependency(token(&format!("d:{i}"))).unwrap();
    }
    assert!(
        feed.broker_started(RequestId::new("p:overflow").unwrap())
            .is_err()
    );
    assert!(feed.register_dependency(token("d:overflow")).is_err());
    feed.register_dependency(token("d:0")).unwrap();
    feed.broker_completed(&RequestId::new("p:0").unwrap(), 1);
    feed.broker_completed(&RequestId::new("p:0").unwrap(), 1);
    assert!(
        feed.broker_started(RequestId::new("p:overflow").unwrap())
            .is_ok()
    );
}

#[test]
fn worker_state_machine_rejects_reordered_calls_and_cancel_is_idempotent() {
    let mut worker = Worker::new(facts());
    assert!(worker.started().is_err());
    assert!(worker.negotiated().is_err());
    assert!(worker.invoke().is_err());
    assert!(worker.finish().is_err());
    assert!(worker.reap(true).is_err());
    worker.enable().unwrap();
    worker.started().unwrap();
    worker.negotiated().unwrap();
    worker.initialized().unwrap();
    worker.invoke().unwrap();
    assert!(worker.admits());
    assert!(worker.invoke().is_err());
    worker.finish().unwrap();
    worker.invoke().unwrap();
    worker.cancel(StopReason::Cancelled).unwrap();
    worker.cancel(StopReason::Stalled).unwrap();
    assert_eq!(
        worker.phase(),
        WorkerPhase::Cancelling(StopReason::Cancelled)
    );
    assert!(!worker.admits());
    worker.exited().unwrap();
    worker.reap(false).unwrap();
    assert!(worker.reap_pending());
    worker.reap(true).unwrap();
    assert!(!worker.reap_pending());
    assert!(worker.trace().contains(ModelAction::LifecycleReap));
}

#[test]
fn hooks_empty_duplicate_reordered_and_cyclic_plans_have_stable_reports() {
    let mut empty = HookRun::plan(&[], &[]).unwrap();
    assert!(empty.next().is_none());
    assert!(empty.may_publish_before());
    assert!(empty.record(true, vec![], vec![]).is_err());
    assert_eq!(empty.report(), empty.report());
    let entries = [
        HookEntry {
            plugin: PluginId::new("p/a").unwrap(),
            required: true,
        },
        HookEntry {
            plugin: PluginId::new("p/b").unwrap(),
            required: false,
        },
    ];
    assert!(HookRun::plan(&[entries[0].clone(), entries[0].clone()], &[]).is_err());
    let edge = (entries[0].plugin.clone(), entries[1].plugin.clone());
    assert!(HookRun::plan(&entries, &[edge.clone(), (edge.1.clone(), edge.0.clone())]).is_err());
    let mut a = HookRun::plan(&entries, &[edge.clone(), edge.clone()]).unwrap();
    let b = HookRun::plan(&[entries[1].clone(), entries[0].clone()], &[edge]).unwrap();
    assert_eq!(a.order(), b.order());
    a.record(false, vec![token("receipt:1")], vec![]).unwrap();
    assert!(!a.next().unwrap().1);
    a.record(true, vec![token("receipt:2")], vec![token("forged")])
        .unwrap();
    assert!(!a.may_publish_before());
    a.published();
    let report = a.report();
    assert_eq!(report, a.report());
    assert_eq!(report.receipts, vec![token("receipt:1")]);
    assert_eq!(report.rejected_claims, vec![token("forged")]);
}

#[test]
fn handle_tables_refuse_max_plus_one_and_revoke_all_state() {
    let mut h = Handles::default();
    assert!(h.get(&token("missing")).is_err());
    assert!(h.upload_mut(&token("missing")).is_err());
    for prefix in ["", "é", "a/b", "\0"] {
        assert!(h.mint(prefix).is_err());
    }
    let identity = FileIdentity {
        object: 1,
        version: 1,
        size: 1,
    };
    let mut first = None;
    for _ in 0..MAX_HANDLES {
        let handle = h
            .register(binding("op:1"), HandleKind::File { path: path() })
            .unwrap();
        let pre = h.precondition(&handle, identity).unwrap();
        assert_eq!(h.check_precondition(&handle, &pre).unwrap(), identity);
        assert!(h.check_precondition(&token("foreign"), &pre).is_err());
        h.start_upload(Upload {
            resource: handle.clone(),
            precondition: pre.clone(),
            invocation: token("op:1"),
            bytes: vec![],
            sealed: true,
        })
        .unwrap();
        first.get_or_insert((handle, pre));
    }
    assert!(h.register(binding("op:1"), HandleKind::Repository).is_err());
    let (handle, pre) = first.unwrap();
    assert!(h.precondition(&handle, identity).is_err());
    h.retire_precondition(&pre);
    assert!(h.check_precondition(&handle, &pre).is_err());
    assert!(h.reclaim_upload(&token("missing")).is_none());
    h.revoke_all();
    h.revoke_all();
    assert!(h.get(&handle).is_err());
    assert!(h.register(binding("op:1"), HandleKind::Repository).is_ok());
}

#[test]
fn broker_raw_content_every_truncation_and_effect_replays_do_not_reexecute() {
    let sample = b"a\xff\0\xc0\x80";
    for end in 0..=sample.len() {
        let mut b = broker();
        let handle = file_handle(&mut b);
        let pre = b.issue_precondition(&handle).unwrap();
        let upload = stage(&mut b, &handle, &pre, &sample[..end]);
        let request = replace_req(&handle, &pre, &upload, "a:1");
        let (key, _) = b.replay_key(&request).unwrap();
        b.handle(&request).unwrap();
        let first_replay = b.handle(&request).unwrap();
        assert_eq!(b.handle(&request).unwrap(), first_replay);
        assert_eq!(b.files().replaces, 1);
        assert_eq!(b.files().content(&path()).unwrap(), &sample[..end]);
        b.rotate(&key).unwrap();
        assert!(b.reconcile(&key, false).is_err());
        b.recover().unwrap();
        b.recover().unwrap();
        b.halt();
        b.halt();
        assert!(b.authority().halted());
        assert_eq!(b.budget().quota(Dimension::Upload).used(), 0);
    }
}

#[test]
fn broker_refuses_foreign_handles_and_overflowing_reads_before_io() {
    let mut b = broker();
    let foreign = b
        .register_bound(binding("op:2"), HandleKind::File { path: path() })
        .unwrap();
    assert!(b.handle(&read_req(&foreign, 0, 1, None)).is_err());
    assert_eq!(b.files().reads, 0);
    let handle = file_handle(&mut b);
    assert!(b.handle(&read_req(&handle, 0, u64::MAX, None)).is_err());
    assert_eq!(b.files().reads, 0);
    assert!(b.handle(&read_req(&token("missing"), 0, 1, None)).is_err());
    let pre = b.issue_precondition(&handle).unwrap();
    assert!(
        b.handle(&Request::FileStage(FileStage {
            resource: handle,
            precondition: pre,
            upload: None,
            offset: u64::MAX,
            bytes: Chunk::new(b"x").unwrap(),
            final_chunk: true
        }))
        .is_err()
    );
    assert_eq!(b.budget().quota(Dimension::Upload).used(), 0);
}

#[test]
fn failed_upload_allocation_and_halt_release_live_byte_quota() {
    let mut b = broker();
    let handle = file_handle(&mut b);
    let pre = b.issue_precondition(&handle).unwrap();
    let request = Request::FileStage(FileStage {
        resource: handle,
        precondition: pre,
        upload: None,
        offset: 0,
        bytes: Chunk::new(b"x").unwrap(),
        final_chunk: true,
    });
    for _ in 0..MAX_HANDLES {
        b.handle(&request).unwrap();
    }
    assert_eq!(
        b.budget().quota(Dimension::Upload).used(),
        MAX_HANDLES as u64
    );
    assert!(b.handle(&request).is_err());
    assert_eq!(
        b.budget().quota(Dimension::Upload).used(),
        MAX_HANDLES as u64
    );
    b.halt();
    assert_eq!(b.budget().quota(Dimension::Upload).used(), 0);
}

#[test]
fn full_ledger_refusal_does_not_consume_action_quota() {
    let mut authority = Authority::new(layers(all_caps()), included(), binding("op:1"), 0);
    authority.initialize().unwrap();
    let mut files = FakeFiles::default();
    files.put(&path(), b"abc");
    let mut b = Broker::new(
        authority,
        Ledger::new(0),
        budget(100, 10),
        1 << 20,
        (FakeAudit::default(), files, FakeProcess::default()),
    );
    let handle = file_handle(&mut b);
    let pre = b.issue_precondition(&handle).unwrap();
    let repository = b.register(HandleKind::Repository).unwrap();
    for _ in 0..2 {
        assert!(
            b.handle(&git_req(&handle, &pre, &repository, "a:1"))
                .is_err()
        );
        assert_eq!(b.budget().quota(Dimension::Actions).used(), 0);
    }
    assert!(b.recipes().recipes.is_empty());
}

#[test]
fn refused_requests_leave_no_phantom_liveness_dependencies_or_capacity_leak() {
    let mut session = HostSession::new(broker(), supervisor(), 1);
    for i in 0..=MAX_TRACKED {
        let id = RequestId::new(&format!("p:{i}")).unwrap();
        session
            .receive(id.clone(), read_req(&token("unknown"), 0, 1, None), 0)
            .unwrap();
        assert!(session.serve_next(0).unwrap().1.is_err());
        assert_eq!(session.queued(), 0);
    }
    assert_eq!(session.tick(1), Verdict::SendProbe);
    assert!(matches!(
        session.probe_reply(
            Some(&Wait::BrokerRequest(RequestId::new("p:0").unwrap())),
            1
        ),
        Verdict::Cancel(_)
    ));
    assert!(session.broker().authority().halted());
    assert!(!session.supervisor().admits());
}

#[test]
fn session_backpressure_and_supervisor_cancellation_use_only_explicit_observations() {
    let mut session = HostSession::new(broker(), supervisor(), 1);
    session
        .receive(
            RequestId::new("p:1").unwrap(),
            read_req(&token("missing"), 0, 1, None),
            0,
        )
        .unwrap();
    assert!(
        session
            .receive(
                RequestId::new("p:2").unwrap(),
                read_req(&token("missing"), 0, 1, None),
                0
            )
            .is_err()
    );
    assert_eq!(session.queued(), 0);
    assert!(session.serve_next(0).is_none());
    session.cancel(StopReason::Cancelled, 0);
    session.tick(1);
    assert_eq!(session.supervisor().process().forced, 1);
    assert_eq!(session.supervisor().process().reaped, 1);
    session.tick(u64::MAX);
    assert_eq!(session.supervisor().process().forced, 1);
}

#[test]
fn immutable_host_values_are_safe_to_share_and_outputs_are_identical() {
    let digest = args();
    let grant = layers(all_caps());
    let selection = included();
    let context = binding("op:1");
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    assert_eq!(
                        ArgDigest::of(b"arguments").unwrap().as_bytes(),
                        digest.as_bytes()
                    );
                    assert_eq!(grant.effective(&selection), grant.effective(&selection));
                    assert_eq!(ActionKey::bind(&context, &token("a")), key("a"));
                    for refusal in [
                        HostRefusal::denied("denied"),
                        HostRefusal::limit("limit"),
                        HostRefusal::conflict("conflict"),
                        HostRefusal::audit("audit"),
                        HostRefusal::unknown(token("receipt:1"), "uncertain"),
                    ] {
                        let wire = refusal.to_wire().unwrap();
                        assert_eq!(wire, refusal.to_wire().unwrap());
                        if wire.effect_state != EffectState::None {
                            assert!(!wire.retryable);
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
    assert!(HostRefusal::denied("invalid reason").to_wire().is_err());
    assert_eq!(HostRefusal::denied("denied").code, ErrorCode::Denied);
}

#[test]
fn every_host_refusal_names_a_next_step_without_changing_wire_metadata() {
    for &code in ErrorCode::ALL {
        for effect in [
            EffectState::None,
            EffectState::Committed,
            EffectState::Partial,
            EffectState::Unknown,
        ] {
            let mut error = HostRefusal::new(code, dctx_plugin_protocol::Phase::Broker, "fixture");
            error.effect_state = effect;
            if effect != EffectState::None {
                error.receipt = Some(token("receipt:1"));
            }
            let display = error.to_string();
            assert!(display.contains("; next: "));
            let wire = error.to_wire().unwrap();
            assert_eq!(wire.code, code);
            assert_eq!(wire.effect_state, effect);
            if effect != EffectState::None {
                assert!(!wire.retryable);
                assert!(display.contains("reconcile"));
            }
        }
    }
}

#[test]
fn exact_recovery_record_ceiling_restores_and_one_extra_refuses_atomically() {
    let mut records = Vec::new();
    for i in 0..MAX_RECOVERY_RECORDS / 2 {
        let mut record = recovery(RecoveryState::Pending);
        record.key = key(&format!("a:{i}"));
        record.receipt = token(&format!("receipt:{}", i + 1));
        records.push(record.clone());
        record.state = RecoveryState::Committed;
        records.push(record);
    }
    let mut ledger = Ledger::new(MAX_RECOVERY_RECORDS / 2);
    ledger.restore(&records).unwrap();
    assert_eq!(
        ledger.committed(&token("op:1")).len(),
        MAX_RECOVERY_RECORDS / 2
    );
    records.push(recovery(RecoveryState::Archived));
    let mut refused = Ledger::new(MAX_RECOVERY_RECORDS);
    assert!(refused.restore(&records).is_err());
    assert!(refused.committed(&token("op:1")).is_empty());
}

#[test]
fn supervisor_public_lifecycle_and_progress_ports_use_injected_observations() {
    let clock = FakeClock::default();
    assert_eq!(clock.now(), 0);
    clock.advance(1);
    clock.advance(u64::MAX);
    assert_eq!(clock.now(), u64::MAX);
    let mut s = supervisor();
    assert!(s.feed().is_some());
    assert!(s.progress(&sample(1, 1), 0).unwrap());
    let dependency = token("dep:1");
    s.register_dependency(dependency.clone()).unwrap();
    s.settle_dependency(&dependency, false, 0);
    let id = RequestId::new("p:1").unwrap();
    s.broker_started(id.clone()).unwrap();
    s.broker_completed(&id, 0);
    s.finish().unwrap();
    assert!(s.feed().is_none());
    assert!(s.progress(&sample(2, 2), 0).is_err());
    s.invoke(0).unwrap();
    assert!(s.progress(&sample(1, 1), 0).unwrap());
    s.cancel(StopReason::Cancelled, 0).unwrap();
    s.cancel(StopReason::Stalled, 1).unwrap();
    s.acknowledged().unwrap();
    assert!(!s.admits());
    assert_eq!(s.process().reaped, 1);
    assert!(s.acknowledged().is_err());
    let mut failed = Supervisor::new(
        facts(),
        POLICY,
        FakeProcess {
            platform_supported: false,
            ..FakeProcess::default()
        },
    );
    assert!(failed.launch().is_err());
    assert_eq!(failed.process().spawns, 0);
}

#[test]
fn program_recipe_success_replay_uncertainty_and_reconciliation_do_not_repeat_effects() {
    for uncertain in [false, true] {
        let mut b = broker();
        let resource = file_handle(&mut b);
        let pre = b.issue_precondition(&resource).unwrap();
        let repository = b.register(HandleKind::Repository).unwrap();
        if uncertain {
            b.recipes_mut().recipe = RecipeOutcome::Uncertain;
        }
        let request = git_req(&resource, &pre, &repository, "a:git");
        let (key, _) = b.replay_key(&request).unwrap();
        let reply = b.handle(&request);
        if uncertain {
            assert_eq!(reply.unwrap_err().effect_state, EffectState::Unknown);
            assert!(b.handle(&request).is_err());
            assert!(b.reconcile(&key, false).is_err());
            b.reconcile(&key, true).unwrap();
        } else {
            assert!(reply.is_ok());
        }
        let replay = b.handle(&request).unwrap();
        assert_eq!(replay, b.handle(&request).unwrap());
        assert_eq!(b.recipes().recipes.len(), 1);
        b.rotate(&key).unwrap();
        assert_eq!(b.ledger().get(&key).unwrap().executions, 1);
    }
}
