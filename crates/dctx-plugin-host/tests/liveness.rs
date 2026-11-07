// SPDX-License-Identifier: Apache-2.0
//! PP8 lifecycle progress under decision 077 with a fake clock and fake process: progress, not timers.
//! Slow advancing work survives; silent, heartbeat-only, dead-dependency and exited workers are probed,
//! classified and cancelled; cancellation closes admission; reap refusal stays visible as reap pending.
//! Traces to Lifecycle.tla `Advance`, `Observe`, `Probe`, `Classify`, `ObserveWait`, `CancelStalled`,
//! `Halt`, `AcknowledgeOrForce`, `Reap`, `ObserveReapPending` and `ElapsedObservation`.

mod common;

use dctx_plugin_host::fakes::{FakeClock, FakeProcess};
use dctx_plugin_host::ports::Clock;
use dctx_plugin_host::{
    LaunchFacts, ModelAction, Observation, Origin, ProcessEvidence, StopReason, Supervisor,
    Verdict, WorkerPhase,
};
use dctx_plugin_protocol::records::capability::Liveness;
use dctx_plugin_protocol::records::common::Wait;
use dctx_plugin_protocol::records::events::{Progress, ProgressUnit};
use dctx_plugin_protocol::records::scalars::{Name, RequestId, Token};

const FAIL_CLOSED: Liveness = Liveness::UnattendedFailClosed {
    sample_ms: 100,
    cancel_grace_ms: 50,
};

fn running(policy: Liveness, clock: &FakeClock) -> Supervisor<FakeProcess> {
    let facts = LaunchFacts {
        origin: Origin::BundledGit,
        verified: true,
        compatible: true,
        confined: false,
    };
    let mut supervisor = Supervisor::new(facts, policy, FakeProcess::default());
    supervisor.launch().expect("launch");
    supervisor.negotiated().expect("negotiated");
    supervisor.initialized().expect("ready");
    supervisor.invoke(clock.now()).expect("invoke");
    supervisor
}

fn progress(seq: u64, completed: u64) -> Progress {
    Progress {
        invocation: Token::new("op:1").expect("op"),
        seq,
        phase: Name::new("scan").expect("phase"),
        completed,
        total: None,
        unit: ProgressUnit::Items,
        wait: None,
    }
}

fn tick(supervisor: &mut Supervisor<FakeProcess>, clock: &FakeClock, ms: u64) -> Verdict {
    clock.advance(ms);
    supervisor.tick(clock.now())
}

#[test]
fn slow_advancing_work_survives_a_long_interval() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    for step in 1..=50u64 {
        clock.advance(3_600_000);
        assert_eq!(
            supervisor.progress(&progress(step, step), clock.now()),
            Ok(true)
        );
        assert_eq!(
            tick(&mut supervisor, &clock, 1),
            Verdict::Continue,
            "step {step}"
        );
    }
    assert_eq!(supervisor.worker().phase(), WorkerPhase::Running);
    let feed = supervisor.feed().expect("feed");
    assert!(
        feed.trace()
            .contains(ModelAction::LifecycleElapsedObservation)
    );
    assert!(
        !supervisor
            .worker()
            .trace()
            .contains(ModelAction::LifecycleCancelStalled)
    );
}

#[test]
fn silent_worker_is_probed_cancelled_forced_and_reaped() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::SendProbe);
    assert_eq!(
        supervisor.feed().map(|f| f.observation().clone()),
        Some(Observation::Probing)
    );
    assert_eq!(
        tick(&mut supervisor, &clock, 100),
        Verdict::Cancel(StopReason::Stalled)
    );
    assert!(!supervisor.admits());
    assert_eq!(tick(&mut supervisor, &clock, 10), Verdict::Continue);
    assert_eq!(supervisor.process().forced, 0);
    tick(&mut supervisor, &clock, 50);
    assert_eq!(supervisor.process().forced, 1);
    assert_eq!(
        supervisor.worker().phase(),
        WorkerPhase::Reaped(StopReason::Stalled)
    );
    let trace = supervisor.worker().trace();
    assert!(trace.precedes(
        ModelAction::LifecycleCancelStalled,
        ModelAction::LifecycleAcknowledgeOrForce
    ));
    assert!(trace.precedes(
        ModelAction::LifecycleAcknowledgeOrForce,
        ModelAction::LifecycleReap
    ));
}

#[test]
fn heartbeat_replay_does_not_keep_a_worker_healthy() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    assert_eq!(supervisor.progress(&progress(1, 5), clock.now()), Ok(true));
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::Continue);
    let mut verdicts = Vec::new();
    for seq in 2..10u64 {
        assert_eq!(
            supervisor.progress(&progress(seq, 5), clock.now()),
            Ok(false)
        );
        verdicts.push(tick(&mut supervisor, &clock, 100));
        if let Some(Verdict::SendProbe) = verdicts.last() {
            verdicts.push(supervisor.probe_reply(None, clock.now()));
        }
        if matches!(verdicts.last(), Some(Verdict::Cancel(_))) {
            break;
        }
    }
    assert_eq!(verdicts.last(), Some(&Verdict::Cancel(StopReason::Stalled)));
}

#[test]
fn responsive_wait_on_live_dependency_continues_until_it_dies() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    let dep = Token::new("dep:1").expect("dep");
    supervisor.register_dependency(dep.clone()).expect("dep");
    let wait = Wait::Dependency(dep.clone());
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::SendProbe);
    assert_eq!(
        supervisor.probe_reply(Some(&wait), clock.now()),
        Verdict::Continue
    );
    for _ in 0..20 {
        assert_eq!(tick(&mut supervisor, &clock, 1_000_000), Verdict::Continue);
    }
    supervisor.settle_dependency(&dep, false, clock.now());
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::SendProbe);
    assert!(
        supervisor
            .feed()
            .expect("feed")
            .trace()
            .contains(ModelAction::LifecycleObserveWait)
    );
    assert_eq!(
        supervisor.probe_reply(Some(&wait), clock.now()),
        Verdict::Cancel(StopReason::Stalled)
    );
}

#[test]
fn resolved_dependency_and_broker_completion_are_useful_progress() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    let dep = Token::new("dep:1").expect("dep");
    supervisor.register_dependency(dep.clone()).expect("dep");
    supervisor.settle_dependency(&dep, true, clock.now());
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::Continue);
    let request = RequestId::new("p:1").expect("id");
    supervisor.broker_started(request.clone()).expect("started");
    supervisor.broker_completed(&request, clock.now());
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::Continue);
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::SendProbe);
}

#[test]
fn exited_worker_without_response_is_stalled_immediately() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    supervisor.process_mut().evidence = ProcessEvidence::Exited;
    assert_eq!(
        tick(&mut supervisor, &clock, 100),
        Verdict::Cancel(StopReason::Stalled)
    );
}

#[test]
fn unknown_activity_is_unresolved_suspicion_not_zero() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    supervisor.process_mut().evidence = ProcessEvidence::Unknown;
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::SendProbe);
    assert_eq!(
        supervisor.probe_reply(None, clock.now()),
        Verdict::Cancel(StopReason::UnresolvedSuspicion)
    );
}

#[test]
fn interactive_policy_asks_the_user_instead_of_killing() {
    let clock = FakeClock::default();
    let policy = Liveness::Interactive {
        sample_ms: 100,
        cancel_grace_ms: 50,
    };
    let mut supervisor = running(policy, &clock);
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::SendProbe);
    assert_eq!(supervisor.probe_reply(None, clock.now()), Verdict::AskUser);
    assert!(supervisor.admits());
    supervisor
        .cancel(StopReason::Cancelled, clock.now())
        .expect("user cancel");
    assert!(!supervisor.admits());
    assert!(
        supervisor
            .worker()
            .trace()
            .contains(ModelAction::LifecycleHalt)
    );
}

#[test]
fn bounded_observation_waits_for_its_inactivity_window() {
    let clock = FakeClock::default();
    let policy = Liveness::UnattendedBoundedObservation {
        sample_ms: 100,
        cancel_grace_ms: 50,
        inactivity_ms: 1_000,
    };
    let mut supervisor = running(policy, &clock);
    assert_eq!(tick(&mut supervisor, &clock, 100), Verdict::SendProbe);
    assert_eq!(supervisor.probe_reply(None, clock.now()), Verdict::Continue);
    assert_eq!(
        tick(&mut supervisor, &clock, 1_000),
        Verdict::Cancel(StopReason::Stalled)
    );
}

#[test]
fn cancellation_refusing_descendant_shows_reap_pending() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    supervisor.process_mut().reapable = false;
    supervisor
        .cancel(StopReason::Cancelled, clock.now())
        .expect("cancel");
    tick(&mut supervisor, &clock, 50);
    assert_eq!(
        supervisor.worker().phase(),
        WorkerPhase::Exited(StopReason::Cancelled)
    );
    assert!(supervisor.worker().reap_pending());
    assert!(
        supervisor
            .worker()
            .trace()
            .contains(ModelAction::LifecycleObserveReapPending)
    );
    supervisor.process_mut().reapable = true;
    tick(&mut supervisor, &clock, 1);
    assert_eq!(
        supervisor.worker().phase(),
        WorkerPhase::Reaped(StopReason::Cancelled)
    );
    assert!(!supervisor.worker().reap_pending());
}

#[test]
fn acknowledged_cancel_reaps_without_force() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    supervisor
        .cancel(StopReason::Cancelled, clock.now())
        .expect("cancel");
    supervisor.acknowledged().expect("acknowledged");
    assert_eq!(supervisor.process().forced, 0);
    assert_eq!(
        supervisor.worker().phase(),
        WorkerPhase::Reaped(StopReason::Cancelled)
    );
}

#[test]
fn completed_invocation_returns_to_ready() {
    let clock = FakeClock::default();
    let mut supervisor = running(FAIL_CLOSED, &clock);
    supervisor.finish().expect("finish");
    assert_eq!(supervisor.worker().phase(), WorkerPhase::Ready);
    assert!(supervisor.progress(&progress(1, 1), clock.now()).is_err());
    supervisor.invoke(clock.now()).expect("next invocation");
}
