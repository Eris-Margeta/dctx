// SPDX-License-Identifier: Apache-2.0
//! PP4 binding/confinement admission and spawn counts with a fake process port (decision 080).
//! Third-party and official-override builds need proven confinement; only the authenticated bundled-git
//! origin may run with brokered, audited effects while OS confinement is pending. Traces to
//! Lifecycle.tla `EnableOrRefuse`, `Start`, `Negotiate`, `Initialize`, `Invoke`.

use dctx_plugin_host::fakes::FakeProcess;
use dctx_plugin_host::{LaunchFacts, ModelAction, Origin, Supervisor, WorkerPhase};
use dctx_plugin_protocol::ErrorCode;
use dctx_plugin_protocol::records::capability::Liveness;

const POLICY: Liveness = Liveness::UnattendedFailClosed { sample_ms: 10, cancel_grace_ms: 10 };

fn supervisor(origin: Origin, confined: bool) -> Supervisor<FakeProcess> {
    Supervisor::new(LaunchFacts { origin, verified: true, compatible: true, confined }, POLICY, FakeProcess::default())
}

#[test]
fn unconfined_third_party_and_override_never_spawn() {
    for origin in [Origin::ThirdParty, Origin::OfficialOverride] {
        let mut refused = supervisor(origin, false);
        assert_eq!(refused.launch().map_err(|e| e.code), Err(ErrorCode::UnsupportedPlatform), "{origin:?}");
        assert_eq!(refused.process().spawns, 0);
        assert_eq!(refused.worker().phase(), WorkerPhase::Refused);
        let mut confined = supervisor(origin, true);
        confined.launch().expect("confined control");
        assert_eq!(confined.process().spawns, 1);
    }
}

#[test]
fn bundled_git_may_run_pending_os_confinement() {
    let mut bundled = supervisor(Origin::BundledGit, false);
    bundled.launch().expect("bundled exception");
    bundled.negotiated().expect("negotiated");
    bundled.initialized().expect("ready");
    bundled.invoke(0).expect("invoke");
    assert!(bundled.admits());
    let trace = bundled.worker().trace();
    for (a, b) in [
        (ModelAction::LifecycleEnableOrRefuse, ModelAction::LifecycleStart),
        (ModelAction::LifecycleStart, ModelAction::LifecycleNegotiate),
        (ModelAction::LifecycleNegotiate, ModelAction::LifecycleInitialize),
        (ModelAction::LifecycleInitialize, ModelAction::LifecycleInvoke),
    ] {
        assert!(trace.precedes(a, b), "{a:?} before {b:?}");
    }
}

#[test]
fn no_provider_on_platform_is_unsupported_not_unrestricted() {
    let mut supervisor = supervisor(Origin::ThirdParty, true);
    supervisor.process_mut().platform_supported = false;
    assert_eq!(supervisor.launch().map_err(|e| e.code), Err(ErrorCode::UnsupportedPlatform));
    assert_eq!(supervisor.process().spawns, 0);
    assert!(!supervisor.admits());
}

#[test]
fn unverified_artifact_never_spawns() {
    let facts = LaunchFacts { origin: Origin::BundledGit, verified: false, compatible: true, confined: true };
    let mut supervisor = Supervisor::new(facts, POLICY, FakeProcess::default());
    assert_eq!(supervisor.launch().map_err(|e| e.code), Err(ErrorCode::IntegrityFailure));
    assert_eq!(supervisor.process().spawns, 0);
}

#[test]
fn invoke_requires_ready() {
    let mut supervisor = supervisor(Origin::BundledGit, false);
    supervisor.launch().expect("launch");
    assert!(supervisor.invoke(0).is_err());
    assert!(supervisor.initialized().is_err());
    supervisor.negotiated().expect("negotiated");
    supervisor.initialized().expect("ready");
    supervisor.invoke(0).expect("control");
}
