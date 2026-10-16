//! Restart reconciliation and bounded startup cleanup.
//! Implements CACHE-v1 recovery: an OS lock proves a session is alive, and startup is one slice.
//! Depends on the writer steps and [`RestartEvidence`]. P8 is `p8_recovery_progress`.

use dctx_adapter_cache::{
    FakeClock, GenerationId, Lifecycle, Quota, RestartEvidence, RootId, SessionId, SliceBudget,
    WriterPhase, STAGING_TTL_MS, STARTUP_MAX_OBJECTS,
};
use std::collections::BTreeSet;

fn life(hard: u64) -> Lifecycle {
    let mut life = Lifecycle::new(Quota::scaled(hard)).expect("quota");
    life.register_root(RootId::new(1)).expect("root");
    life
}

fn root() -> RootId {
    RootId::new(1)
}

fn evidence(boot: u64, held: &[SessionId]) -> RestartEvidence {
    RestartEvidence { boot_id: boot, held_sessions: held.iter().copied().collect::<BTreeSet<_>>() }
}

fn finish_writer(life: &mut Lifecycle, session: SessionId, bytes: u64) -> GenerationId {
    life.writer_acquire(root(), session).expect("acquire");
    life.writer_reserve(root(), bytes).expect("reserve");
    let segment = bytes / 2;
    life.writer_stage_segment(root(), segment).expect("segment");
    life.writer_validate_segment(root()).expect("segment");
    life.writer_stage_manifest(root(), bytes - segment, &[]).expect("manifest");
    life.writer_validate_manifest(root()).expect("manifest");
    life.writer_swap(root()).expect("swap");
    let generation = life.current_generation(root()).expect("current");
    life.writer_release(root()).expect("release");
    generation
}

#[test]
fn p8_recovery_progress() {
    crash_before_swap_drops_the_temp();
    crash_after_swap_keeps_the_generation();
    held_os_lock_keeps_the_writer_dead();
    boot_change_reclaims_only_unowned_staging();
    publish_completes_or_cache_limit();
    startup_cursor_resumes_within_the_object_budget();
    startup_stops_on_the_time_budget();
}

fn crash_before_swap_drops_the_temp() {
    let mut life = life(100);
    life.writer_acquire(root(), SessionId::new(1)).expect("acquire");
    life.writer_reserve(root(), 10).expect("reserve");
    life.writer_stage_segment(root(), 6).expect("stage");
    life.crash_writer(root()).expect("crash");
    assert_eq!(life.writer_phase(root()), WriterPhase::Dead);
    assert!(life.admitted() > 0);
    life.reconcile(&evidence(0, &[]));
    assert_eq!(life.writer_phase(root()), WriterPhase::Free);
    assert_eq!(life.admitted(), 0);
    assert!(life.current_generation(root()).is_none());
    assert!(life.p3_holds() && life.p4_holds());
}

fn crash_after_swap_keeps_the_generation() {
    let mut life = life(200);
    let first = finish_writer(&mut life, SessionId::new(2), 20);
    life.writer_acquire(root(), SessionId::new(3)).expect("acquire");
    life.writer_reserve(root(), 20).expect("reserve");
    life.writer_stage_segment(root(), 12).expect("segment");
    life.writer_validate_segment(root()).expect("segment");
    life.writer_stage_manifest(root(), 8, &[]).expect("manifest");
    life.writer_validate_manifest(root()).expect("manifest");
    life.writer_swap(root()).expect("swap");
    let second = life.current_generation(root()).expect("current");
    assert_ne!(first, second);
    life.crash_writer(root()).expect("crash");
    assert_eq!(life.writer_phase(root()), WriterPhase::Dead);
    life.reconcile(&evidence(0, &[]));
    assert_eq!(life.writer_phase(root()), WriterPhase::Free);
    assert_eq!(life.current_generation(root()), Some(second));
    assert_eq!(life.admitted(), 40);
    assert!(life.p3_holds() && life.p4_holds());
}

fn held_os_lock_keeps_the_writer_dead() {
    let mut life = life(100);
    let session = SessionId::new(7);
    life.writer_acquire(root(), session).expect("acquire");
    life.writer_reserve(root(), 10).expect("reserve");
    let staged = life.writer_stage_segment(root(), 6).expect("stage");
    life.crash_writer(root()).expect("crash");
    life.reconcile(&evidence(0, &[session]));
    assert_eq!(life.writer_phase(root()), WriterPhase::Dead);
    assert_eq!(life.object_bytes(staged), Some(6));
    life.reconcile(&evidence(0, &[]));
    assert_eq!(life.writer_phase(root()), WriterPhase::Free);
    assert!(life.object_bytes(staged).is_none());
    assert_eq!(life.admitted(), 0);
    assert!(life.p4_holds());
}

fn boot_change_reclaims_only_unowned_staging() {
    let mut life = life(1_000);
    let staged = life.load_staged(5, 0).expect("staged");
    let fact = life.insert_fact(9, &[root()]).expect("fact");
    life.writer_acquire(root(), SessionId::new(4)).expect("acquire");
    life.writer_reserve(root(), 20).expect("reserve");
    let owned = life.writer_stage_segment(root(), 8).expect("owned");
    life.crash_writer(root()).expect("crash");
    life.reconcile(&evidence(9, &[SessionId::new(4)]));
    assert!(life.object_bytes(staged).is_none());
    assert_eq!(life.object_bytes(fact), Some(9));
    assert_eq!(life.object_bytes(owned), Some(8));
    assert_eq!(life.writer_phase(root()), WriterPhase::Dead);
    assert_eq!(life.boot_id(), 9);
    assert!(life.p3_holds() && life.p4_holds());
}

fn publish_completes_or_cache_limit() {
    let mut life = life(10);
    life.writer_acquire(root(), SessionId::new(1)).expect("acquire");
    assert!(life.writer_reserve(root(), 11).is_err());
    assert_eq!(life.writer_phase(root()), WriterPhase::Free);
    assert_eq!(life.admitted(), 0);
    life.writer_acquire(root(), SessionId::new(1)).expect("acquire");
    life.writer_reserve(root(), 10).expect("reserve");
    life.writer_stage_segment(root(), 6).expect("segment");
    life.writer_validate_segment(root()).expect("segment");
    life.writer_stage_manifest(root(), 4, &[]).expect("manifest");
    life.writer_validate_manifest(root()).expect("manifest");
    assert!(life.current_generation(root()).is_none());
    life.writer_swap(root()).expect("swap");
    life.writer_release(root()).expect("release");
    assert!(life.current_generation(root()).is_some());
    assert_eq!(life.writer_phase(root()), WriterPhase::Free);
    assert_eq!(life.admitted(), 10);
    assert!(life.p3_holds() && life.p4_holds());
}

fn startup_cursor_resumes_within_the_object_budget() {
    let mut life = Lifecycle::new(Quota::scaled(1_000)).expect("quota");
    for _ in 0..200 {
        life.load_staged(1, 0).expect("staged");
    }
    let clock = FakeClock::at(STAGING_TTL_MS);
    let first = life.startup(clock.sample(), None).expect("startup");
    assert_eq!(first.examined, STARTUP_MAX_OBJECTS);
    assert!(!first.completed_pass);
    assert_eq!(life.cursor(), u64::from(STARTUP_MAX_OBJECTS) + 1);
    assert_eq!(life.admitted(), 200 - u64::from(STARTUP_MAX_OBJECTS));
    let second = life.startup(clock.sample(), None).expect("startup");
    assert_eq!(second.examined, 200 - STARTUP_MAX_OBJECTS);
    assert!(second.completed_pass);
    assert_eq!(life.admitted(), 0);
    assert_eq!(life.cursor(), 0);
    assert!(life.p4_holds());
}

fn startup_stops_on_the_time_budget() {
    let clock = FakeClock::at(STAGING_TTL_MS);
    let mut life = Lifecycle::new(Quota::scaled(1_000)).expect("quota");
    for _ in 0..20 {
        life.load_staged(1, 0).expect("staged");
    }
    let report = life
        .startup_with(
            clock.sample(),
            None,
            SliceBudget { max_objects: 128, max_millis: 25, millis_per_object: 5 },
        )
        .expect("startup");
    assert_eq!(report.examined, 5);
    assert!(!report.completed_pass);

    let mut life = Lifecycle::new(Quota::scaled(1_000)).expect("quota");
    for _ in 0..20 {
        life.load_staged(1, 0).expect("staged");
    }
    let report = life
        .startup_with(
            clock.sample(),
            None,
            SliceBudget { max_objects: 128, max_millis: 25, millis_per_object: 30 },
        )
        .expect("startup");
    assert_eq!(report.examined, 1);
}
