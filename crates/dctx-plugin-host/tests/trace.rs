// SPDX-License-Identifier: Apache-2.0
//! Model correspondence: every PL-M action the host refines is reached by a real host transition.
//! Drives broker, budget, ledger, supervisor, liveness feed and hook run over fakes and asserts that the
//! union of their traces covers `ModelAction::ALL` (docs/formal/dctx-plugins/README.md correspondence).

mod common;

use std::collections::BTreeSet;

use common::*;
use dctx_plugin_host::fakes::{FakeClock, FakeProcess};
use dctx_plugin_host::ports::{AuditKind, Clock};
use dctx_plugin_host::{
    HandleKind, HookEntry, HookRun, LaunchFacts, Layer, ModelAction, Origin, StopReason, Supervisor,
};
use dctx_plugin_protocol::records::capability::Liveness;
use dctx_plugin_protocol::records::common::Wait;
use dctx_plugin_protocol::records::events::{Progress, ProgressUnit};
use dctx_plugin_protocol::records::scalars::{Name, PluginId, Token};

fn collect(into: &mut BTreeSet<String>, actions: &[ModelAction]) {
    into.extend(actions.iter().map(|a| format!("{a:?}")));
}

fn running(clock: &FakeClock) -> Supervisor<FakeProcess> {
    let facts = LaunchFacts {
        origin: Origin::BundledGit,
        verified: true,
        compatible: true,
        confined: false,
    };
    let policy = Liveness::UnattendedFailClosed {
        sample_ms: 10,
        cancel_grace_ms: 5,
    };
    let mut supervisor = Supervisor::new(facts, policy, FakeProcess::default());
    supervisor.launch().expect("launch");
    supervisor.negotiated().expect("negotiated");
    supervisor.initialized().expect("ready");
    supervisor.invoke(clock.now()).expect("invoke");
    supervisor
}

#[test]
fn every_model_action_is_reached_by_a_host_transition() {
    let mut seen = BTreeSet::new();

    let mut effects = broker();
    let handle = file_handle(&mut effects);
    let repo = effects.register(HandleKind::Repository).expect("repo");
    let pre = first_read(&mut effects, &handle);
    effects
        .handle(&git_req(&handle, &pre, &repo, "g:1"))
        .expect("admin");
    let upload = stage(&mut effects, &handle, &pre, b"abc\nx");
    let request = replace_req(&handle, &pre, &upload, "a:1");
    effects.handle(&request).expect("commit");
    effects.handle(&request).expect("repeat");
    effects.audit_mut().fail_next = Some(AuditKind::Intent);
    assert!(replace_with(&mut effects, b"abc\nxy", "a:2").is_err());
    effects.audit_mut().fail_next = Some(AuditKind::Completion);
    let handle2 = file_handle(&mut effects);
    let pre2 = first_read(&mut effects, &handle2);
    let upload2 = stage(&mut effects, &handle2, &pre2, b"abc\nxyz");
    let uncertain = replace_req(&handle2, &pre2, &upload2, "a:3");
    assert!(effects.handle(&uncertain).is_err());
    let (key, _) = effects.replay_key(&uncertain).expect("key");
    effects.reconcile(&key, true).expect("reconcile");
    effects.rotate(&key).expect("rotate");
    collect(&mut seen, effects.authority().trace().actions());
    collect(&mut seen, effects.ledger().trace().actions());
    collect(&mut seen, effects.budget().trace().actions());

    let mut policy = broker();
    policy.authority_mut().configure(&all_caps());
    policy.authority_mut().approve(Layer::Consent, all_caps());
    policy
        .authority_mut()
        .narrow(Layer::Organization, &all_caps());
    policy.authority_mut().change_selection(included());
    policy.halt();
    collect(&mut seen, policy.authority().trace().actions());

    let clock = FakeClock::default();
    let mut stalled = running(&clock);
    let sample = Progress {
        invocation: token("op:1"),
        seq: 1,
        phase: Name::new("scan").expect("phase"),
        completed: 1,
        total: None,
        unit: ProgressUnit::Items,
        wait: None,
    };
    stalled.progress(&sample, clock.now()).expect("advance");
    let dep = Token::new("dep:1").expect("dep");
    stalled.register_dependency(dep.clone()).expect("dep");
    clock.advance(10);
    stalled.tick(clock.now());
    clock.advance(10);
    stalled.tick(clock.now());
    stalled.probe_reply(Some(&Wait::Dependency(dep.clone())), clock.now());
    stalled.settle_dependency(&dep, false, clock.now());
    clock.advance(10);
    stalled.tick(clock.now());
    stalled.probe_reply(None, clock.now());
    stalled.process_mut().reapable = false;
    clock.advance(10);
    stalled.tick(clock.now());
    stalled.process_mut().reapable = true;
    clock.advance(1);
    stalled.tick(clock.now());
    collect(&mut seen, stalled.worker().trace().actions());
    collect(&mut seen, stalled.feed().expect("feed").trace().actions());

    let mut halted = running(&clock);
    halted.finish().expect("finish");
    halted
        .cancel(StopReason::Cancelled, clock.now())
        .expect("halt");
    collect(&mut seen, halted.worker().trace().actions());

    let plugin = PluginId::new("dctx/git").expect("id");
    let mut run = HookRun::plan(
        &[HookEntry {
            plugin,
            required: false,
        }],
        &[],
    )
    .expect("plan");
    run.record(true, vec![], vec![]).expect("run");
    run.published();
    run.report();
    collect(&mut seen, run.trace().actions());

    let missing: Vec<String> = ModelAction::ALL
        .iter()
        .map(|a| format!("{a:?}"))
        .filter(|name| !seen.contains(name))
        .collect();
    assert!(missing.is_empty(), "unreached model actions: {missing:?}");
}
