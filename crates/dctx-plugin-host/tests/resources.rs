// SPDX-License-Identifier: Apache-2.0
//! PP7 bounded resources and backpressure: exact budgets, frame overhead, queues that cancel the sender.
//! Traces to Resources.tla `Receive`/`Drain` and Lifecycle.tla `Halt` on overflow. Each refusal has a
//! positive control; a quota property test checks the minimum ceiling and non-wrapping sums.

mod common;

use common::*;
use dctx_plugin_host::fakes::FakeProcess;
use dctx_plugin_host::{
    Budget, Dimension, HostSession, LaunchFacts, ModelAction, Origin, Quota, StopReason,
    Supervisor, WorkerPhase,
};
use dctx_plugin_protocol::ErrorCode;
use dctx_plugin_protocol::records::capability::Liveness;
use dctx_plugin_protocol::records::scalars::RequestId;
use proptest::prelude::*;

const POLICY: Liveness = Liveness::UnattendedFailClosed {
    sample_ms: 10,
    cancel_grace_ms: 5,
};

fn session(
    outstanding: usize,
) -> HostSession<
    dctx_plugin_host::fakes::FakeAudit,
    dctx_plugin_host::fakes::FakeFiles,
    FakeProcess,
    FakeProcess,
> {
    let facts = LaunchFacts {
        origin: Origin::BundledGit,
        verified: true,
        compatible: true,
        confined: false,
    };
    let mut supervisor = Supervisor::new(facts, POLICY, FakeProcess::default());
    supervisor.launch().expect("launch");
    supervisor.negotiated().expect("negotiated");
    supervisor.initialized().expect("ready");
    supervisor.invoke(0).expect("invoke");
    HostSession::new(broker(), supervisor, outstanding)
}

fn id(n: u32) -> RequestId {
    RequestId::new(&format!("p:{n}")).expect("id")
}

#[test]
fn exact_content_budget_succeeds_and_one_more_byte_refuses() {
    let mut broker = broker_with(layers(all_caps()), included(), budget(3, 8));
    let handle = file_handle(&mut broker);
    assert!(broker.handle(&read_req(&handle, 0, 3, None)).is_ok());
    assert_eq!(broker.budget().quota(Dimension::Content).used(), 3);
    let refused = broker
        .handle(&read_req(&handle, 0, 1, None))
        .expect_err("over budget");
    assert_eq!(refused.code, ErrorCode::ResourceLimit);
    assert_eq!(broker.files().reads, 1);
    assert!(
        broker
            .budget()
            .trace()
            .contains(ModelAction::ResourcesReceive)
    );
}

#[test]
fn short_reads_charge_only_delivered_bytes() {
    let mut broker = broker_with(layers(all_caps()), included(), budget(64, 8));
    let handle = file_handle(&mut broker);
    broker
        .handle(&read_req(&handle, 0, 64, None))
        .expect("short read");
    assert_eq!(broker.budget().quota(Dimension::Content).used(), 3);
}

#[test]
fn chunk_must_fit_frame_after_base64_overhead() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let mut small = broker_with(layers(all_caps()), included(), budget(1 << 20, 8));
    let small_handle = file_handle(&mut small);
    assert!(
        small
            .handle(&read_req(&small_handle, 0, 65_536, None))
            .is_ok()
    );
    let mut tight = dctx_plugin_host::Broker::new(
        {
            let mut a = dctx_plugin_host::Authority::new(
                layers(all_caps()),
                included(),
                binding("op:1"),
                0,
            );
            a.initialize().expect("ready");
            a
        },
        dctx_plugin_host::Ledger::new(8),
        budget(1 << 20, 8),
        65_536,
        (
            dctx_plugin_host::fakes::FakeAudit::default(),
            dctx_plugin_host::fakes::FakeFiles::default(),
            FakeProcess::default(),
        ),
    );
    let tight_handle = tight
        .register(dctx_plugin_host::HandleKind::File { path: path() })
        .expect("handle");
    let refused = tight
        .handle(&read_req(&tight_handle, 0, 65_536, None))
        .expect_err("frame");
    assert_eq!(refused.reason, "chunk-exceeds-frame");
    assert_eq!(tight.files().identities, 0);
    assert!(broker.handle(&read_req(&handle, 0, 3, None)).is_ok());
}

#[test]
fn action_quota_bounds_effects() {
    let mut broker = broker_with(layers(all_caps()), included(), budget(1 << 20, 1));
    replace_with(&mut broker, b"abc\nx", "a:1").expect("first action");
    let refused = replace_with(&mut broker, b"abc\nxy", "a:2").expect_err("second action");
    assert_eq!(refused.code, ErrorCode::ResourceLimit);
    assert_eq!(broker.files().replaces, 1);
}

#[test]
fn queue_overflow_cancels_the_sender_and_closes_admission() {
    let mut session = session(2);
    let handle = file_handle(session.broker_mut());
    session
        .receive(id(1), read_req(&handle, 0, 3, None), 1)
        .expect("one");
    session
        .receive(id(2), read_req(&handle, 0, 3, None), 1)
        .expect("two");
    let refused = session
        .receive(id(3), read_req(&handle, 0, 3, None), 1)
        .expect_err("overflow");
    assert_eq!(refused.code, ErrorCode::ResourceLimit);
    assert_eq!(session.queued(), 0);
    assert_eq!(
        session.supervisor().worker().phase(),
        WorkerPhase::Cancelling(StopReason::ResourceLimit)
    );
    assert!(
        session
            .receive(id(4), read_req(&handle, 0, 3, None), 1)
            .is_err()
    );
    assert!(session.broker().authority().halted());
    assert_eq!(session.broker().files().reads, 0);
}

#[test]
fn queue_within_limit_is_served_in_order_control() {
    let mut session = session(2);
    let handle = file_handle(session.broker_mut());
    session
        .receive(id(1), read_req(&handle, 0, 1, None), 1)
        .expect("one");
    session
        .receive(id(2), read_req(&handle, 0, 2, None), 1)
        .expect("two");
    let (first, reply) = session.serve_next(2).expect("served");
    assert_eq!(first, id(1));
    assert!(reply.is_ok());
    session
        .receive(id(3), read_req(&handle, 0, 3, None), 2)
        .expect("room again");
    assert_eq!(session.queued(), 2);
    assert!(session.supervisor().admits());
}

#[test]
fn budget_rejects_all_dimensions_atomically() {
    let mut budget = Budget::new(&[2], &[1], &[0], &[0], &[1]);
    assert!(
        budget
            .charge(&[(Dimension::Content, 2), (Dimension::Upload, 1)])
            .is_err()
    );
    assert_eq!(budget.quota(Dimension::Content).used(), 0);
    budget.charge(&[(Dimension::Content, 2)]).expect("exact");
}

proptest! {
    /// PP7: the limit is the minimum ceiling; accepted charges sum exactly and never exceed it.
    #[test]
    fn quota_never_exceeds_minimum(ceilings in proptest::collection::vec(0u64..64, 1..5),
                                   charges in proptest::collection::vec(0u64..u64::MAX, 0..20)) {
        let mut quota = Quota::new(&ceilings);
        let limit = *ceilings.iter().min().expect("non-empty");
        prop_assert_eq!(quota.limit(), limit);
        let mut sum = 0u64;
        for charge in charges {
            let n = charge % 32;
            if quota.charge(n).is_ok() { sum += n; }
            prop_assert!(quota.used() <= limit);
            prop_assert_eq!(quota.used(), sum);
        }
        prop_assert!(quota.charge(u64::MAX).is_err() || limit == u64::MAX);
    }
}
