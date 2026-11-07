// SPDX-License-Identifier: Apache-2.0
use super::*;
use proptest::prelude::*;

fn publish(world: &mut World) {
    assert!(world.acquire());
    assert!(world.reserve());
    assert!(world.write_seg());
    assert!(world.validate_seg());
    assert!(world.write_man());
    assert!(world.validate_man());
    assert!(world.swap());
    assert!(world.release());
}

proptest! {
    #[test]
    fn random_steps_keep_p2_and_p3(steps in prop::collection::vec(0u8..20, 0..48)) {
        let mut world = World::new(Mutation::None, true);
        for step in steps {
            world.apply(step);
            prop_assert!(world.p2());
            prop_assert!(world.p3());
            if world.lock == Lock::Dead {
                let mut recovered = world.clone();
                prop_assert!(recovered.recover());
                prop_assert_eq!(recovered.lock, Lock::Free);
            }
        }
    }
}

#[test]
fn pointer_first_mutation_breaks_p2_and_the_safe_order_does_not() {
    let mut bad = World::new(Mutation::PointerFirst, true);
    assert!(bad.acquire() && bad.reserve() && bad.write_seg() && bad.swap());
    assert!(!bad.p2());
    let mut good = World::new(Mutation::None, true);
    assert!(good.acquire() && good.reserve() && good.write_seg());
    assert!(!good.swap());
    assert!(
        good.validate_seg()
            && good.write_man()
            && good.validate_man()
            && good.swap()
            && good.release()
    );
    assert!(good.p2());
}

#[test]
fn omit_leases_mutation_breaks_p3_and_a_live_pin_does_not() {
    let mut bad = World::new(Mutation::OmitLeases, true);
    assert!(bad.reader_begin() && bad.reader_confirm());
    publish(&mut bad);
    assert!(
        bad.drop_prev() && bad.retire() && bad.arm_grace() && bad.to_pending() && bad.to_gone()
    );
    assert!(bad.lease_holds_old());
    assert!(!bad.p3());

    let mut good = World::new(Mutation::None, true);
    assert!(good.reader_begin() && good.reader_confirm());
    publish(&mut good);
    assert!(good.drop_prev());
    assert!(!good.retire());
    assert!(good.p3());
    assert!(
        good.reader_release()
            && good.retire()
            && good.arm_grace()
            && good.to_pending()
            && good.to_gone()
    );
    assert!(good.p3());
    assert_eq!(good.old_kept, Kept::Gone);
}

#[test]
fn fair_progress_publishes_or_refuses_and_stuck_writer_does_not() {
    assert_eq!(World::fair(Mutation::None, true, false), "published");
    assert_eq!(World::fair(Mutation::None, true, true), "published");
    assert_eq!(World::fair(Mutation::None, false, false), "refused");
    assert_eq!(World::fair(Mutation::StuckWriter, true, true), "stuck");
}
