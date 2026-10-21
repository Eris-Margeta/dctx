#![allow(clippy::drop_non_drop)]
//! Property test of Install.tla PP9Integrity over random install/activate/fault/revoke/rollback runs:
//! the active version is always complete, recorded and below or at the high-water mark, admission never
//! leases a revoked or quarantined digest, and no refusal leaves staging behind.

mod common;

use common::*;
use dctx_plugin_distribution::fakes::Fault;
use dctx_plugin_distribution::{Confinement, ConsentApproval, ConsentScope, InstallKey, Store};
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    Install(u64),
    Activate(u64),
    Crash(u64, u8),
    Revoke(u64),
    Rollback(u64),
    Enable,
    Disable,
    Admit,
    Recover,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (1u64..5).prop_map(Op::Install),
        (1u64..5).prop_map(Op::Activate),
        ((1u64..5), 0u8..3).prop_map(|(n, f)| Op::Crash(n, f)),
        (1u64..5).prop_map(Op::Revoke),
        (1u64..5).prop_map(Op::Rollback),
        Just(Op::Enable),
        Just(Op::Disable),
        Just(Op::Admit),
        Just(Op::Recover),
    ]
}

fn release(n: u64) -> Release {
    Release::git(&format!("{n}.0.0"), n)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn pp9_integrity_holds_on_every_run(ops in proptest::collection::vec(op(), 1..24)) {
        let mut world = World::new();
        for n in 1..5 {
            world.publish(&release(n), &world.official);
        }
        let git = id(GIT);
        for op in ops {
            let dist = world.dist();
            match &op {
                Op::Install(n) => { let _ = world.install(&release(*n)); }
                Op::Activate(n) => { let _ = dist.activate(&git, &release(*n).digest()); }
                Op::Crash(n, f) => {
                    world.store.inject([Fault::Stage, Fault::Publish, Fault::SaveRegistry][*f as usize]);
                    let _ = world.install(&release(*n));
                    let _ = dist.recover();
                }
                Op::Revoke(n) => { drop(dist); world.trust.record_revocations([], [release(*n).digest()], NOW); }
                Op::Rollback(n) => { let _ = dist.rollback(&git, &release(*n).digest(), "property run"); }
                Op::Enable => {
                    let caps = dist.registry().unwrap().get(&git).and_then(|r| r.active_version().map(|v| (v.digest, v.capabilities.clone())));
                    if let Some((digest, capabilities)) = caps {
                        let _ = dist.enable(&git, Some(&ConsentApproval { digest, capabilities, scope: ConsentScope::User }), Confinement::Established);
                    }
                }
                Op::Disable => { let _ = dist.disable(&git); }
                Op::Admit => {
                    if let Ok(admission) = dist.admit(&git) {
                        prop_assert!(!world.trust.is_artifact_revoked(&admission.digest));
                        dist.release(&admission).unwrap();
                    }
                }
                Op::Recover => { let _ = dist.recover(); }
            }
            let registry = world.dist().registry().unwrap();
            prop_assert!(world.store.staged().unwrap().is_empty(), "staging left after {op:?}");
            if let Some(record) = registry.get(&git) {
                if let Some(active) = record.active {
                    let version = record.version(&active).expect("active is recorded");
                    prop_assert!(version.release_sequence <= record.high_water);
                    prop_assert!(version.release_sequence == record.high_water || !record.rollbacks.is_empty());
                    let key = InstallKey { id: git.clone(), version: version.version.clone(), digest: active };
                    prop_assert!(world.store.installed_files(&key).is_some(), "active is a complete directory");
                }
                prop_assert!(record.leases.is_empty());
            }
        }
    }
}
