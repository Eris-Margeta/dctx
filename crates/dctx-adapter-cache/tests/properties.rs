//! Random schedules for P3 and P4, and a source scan for process and version-control use.
//! Implements the DX-A2 invariants across publish, lease, gc, restart and admission.
//! Depends on `proptest` and the public ledger. Production sources are included as text.

use dctx_adapter_cache::{
    FakeClock, Lifecycle, Purpose, Quota, RestartEvidence, RootId, SessionId,
};
use dctx_core::ids::Timestamp;
use proptest::prelude::*;
use std::collections::BTreeSet;

fn drive(steps: Vec<u8>) -> Result<(), TestCaseError> {
    let mut life = Lifecycle::new(Quota::scaled(80)).expect("quota");
    let left = RootId::new(1);
    let right = RootId::new(2);
    life.register_root(left).expect("root");
    life.register_root(right).expect("root");
    let mut clock = FakeClock::at(0);
    let mut lease = None;
    for step in steps {
        let root = if step % 2 == 0 { left } else { right };
        clock.advance(u64::from(step) * 3_600_000);
        life.follow(&clock);
        if step == 3 && clock.sample().wall > Timestamp(0) {
            let before = life.wall();
            clock.rewind_wall(1);
            life.follow(&clock);
            prop_assert!(life.clock_fault());
            prop_assert_eq!(life.wall(), before);
            clock.advance(1);
            life.follow(&clock);
        }
        match step % 11 {
            0 => {
                let _ = life.touch(root);
            }
            1 => {
                let _ = life.publish_view(root, 8, &[]);
            }
            2 => {
                let _ = life.publish_overlay(root, 8, &[]);
            }
            3 => {
                let _ = life.insert_fact(8, &[root]);
            }
            4 => {
                if let Ok(id) = life.reader_begin(root, SessionId::new(1)) {
                    lease = Some(id);
                }
            }
            5 => {
                if let Some(id) = lease {
                    let _ = life.reader_confirm(id);
                }
            }
            6 => {
                if let Some(id) = lease.take() {
                    let _ = life.reader_release(id);
                }
            }
            7 => {
                life.gc(dctx_adapter_cache::GcRequest {
                    root: None,
                    budget: dctx_adapter_cache::SliceBudget::explicit(16),
                    bypass_grace: step % 2 == 0,
                    max_bytes: None,
                    cancel: None,
                })
                .expect("gc");
            }
            8 => {
                let _ = life.startup(
                    clock.sample(),
                    Some(&RestartEvidence { boot_id: life.boot_id(), held_sessions: BTreeSet::new() }),
                );
            }
            9 => {
                if let Ok(hold) = life.reserve_for(Some(root), Purpose::Payload, 8) {
                    if step % 2 == 0 {
                        life.abort_reservation(hold);
                    } else {
                        let _ = life.commit_reservation(hold, 4);
                    }
                }
            }
            _ => {
                if life.writer_acquire(root, SessionId::new(9)).is_ok()
                    && life.writer_reserve(root, 12).is_ok()
                {
                    let _ = life.writer_stage_segment(root, 6);
                    let _ = life.crash_writer(root);
                    life.reconcile(&RestartEvidence {
                        boot_id: life.boot_id(),
                        held_sessions: BTreeSet::new(),
                    });
                }
            }
        }
        prop_assert!(life.p3_holds());
        prop_assert!(life.p4_holds());
        prop_assert_eq!(life.buckets().admitted(), life.admitted());
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 48,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn p3_and_p4_hold_across_random_schedules(steps in proptest::collection::vec(0u8..11, 1..20)) {
        drive(steps)?;
    }
}

#[test]
fn sources_do_not_invoke_git_or_start_a_thread() {
    const SOURCES: &[&str] = &[
        include_str!("../src/lib.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/adapters/mod.rs"),
        include_str!("../src/adapters/cache.rs"),
        include_str!("../src/ports/mod.rs"),
        include_str!("../src/domain/mod.rs"),
        include_str!("../src/domain/clock.rs"),
        include_str!("../src/domain/compact.rs"),
        include_str!("../src/domain/ids.rs"),
        include_str!("../src/domain/lease.rs"),
        include_str!("../src/domain/legacy.rs"),
        include_str!("../src/domain/maintain.rs"),
        include_str!("../src/domain/publish.rs"),
        include_str!("../src/domain/purge.rs"),
        include_str!("../src/domain/quota.rs"),
        include_str!("../src/domain/reach.rs"),
        include_str!("../src/domain/reserve.rs"),
        include_str!("../src/domain/restart.rs"),
        include_str!("../src/domain/status.rs"),
        include_str!("../src/domain/writer.rs"),
    ];
    let dot_git: String = ['.', 'g', 'i', 't'].into_iter().collect();
    for source in SOURCES {
        assert!(!source.contains("std::process"), "process spawn");
        assert!(!source.contains("std::thread"), "thread");
        assert!(!source.contains("Command::"), "command");
        assert!(!source.contains(dot_git.as_str()), "version-control metadata");
    }
}
