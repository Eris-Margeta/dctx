//! Finite publication, pin, and reclamation state machine.
//! Implements CacheLifecycle.tla actions for P2, P3, and P8 (DX-A2), without git ownership.
//! Depends on no IO. Property tests drive it; [`crate::SegmentStore`] is the filesystem image of the same cuts.
//! `PointerFirst`, `OmitLeases`, and `StuckWriter` are the falsifying mutations, not the pass path.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lock {
    Free,
    Held,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Idle,
    Held,
    Reserved,
    Staged,
    Manifest,
    Ready,
    Swapped,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    None,
    Old,
    New,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Obj {
    Absent,
    Temp,
    Valid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kept {
    Live,
    Retired,
    Pending,
    Gone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Acquiring,
    Active,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mutation {
    None,
    PointerFirst,
    OmitLeases,
    StuckWriter,
}

#[derive(Clone, Debug)]
struct World {
    lock: Lock,
    step: Step,
    ptr: Which,
    prev: Which,
    pointed: bool,
    seg: Obj,
    man: Obj,
    old_kept: Kept,
    grace: u8,
    lease_phase: Phase,
    lease: Which,
    observed: Which,
    acquires: u8,
    crashes_left: u8,
    mutation: Mutation,
    cap_allows: bool,
}

impl World {
    fn new(mutation: Mutation, cap_allows: bool) -> Self {
        Self {
            lock: Lock::Free,
            step: Step::Idle,
            ptr: Which::Old,
            prev: Which::None,
            pointed: false,
            seg: Obj::Absent,
            man: Obj::Absent,
            old_kept: Kept::Live,
            grace: 0,
            lease_phase: Phase::Idle,
            lease: Which::None,
            observed: Which::None,
            acquires: 0,
            crashes_left: 2,
            mutation,
            cap_allows,
        }
    }

    fn p2(&self) -> bool {
        let visible = (self.ptr == Which::New) == self.pointed;
        let complete = !self.pointed || (self.seg == Obj::Valid && self.man == Obj::Valid);
        let new_lease = self.lease != Which::New
            || self.lease_phase != Phase::Active
            || (self.pointed && self.seg == Obj::Valid && self.man == Obj::Valid);
        let one = self.lease_phase != Phase::Active || matches!(self.lease, Which::Old | Which::New);
        visible && complete && new_lease && one
    }

    fn lease_holds_old(&self) -> bool {
        self.lease_phase == Phase::Active && self.lease == Which::Old
    }

    fn p3(&self) -> bool {
        let pinned = !self.lease_holds_old() || matches!(self.old_kept, Kept::Live | Kept::Retired);
        let current = self.ptr != Which::Old || self.old_kept != Kept::Gone;
        let recovery = self.prev != Which::Old || matches!(self.old_kept, Kept::Live | Kept::Retired);
        pinned && current && recovery
    }

    fn acquire(&mut self) -> bool {
        if self.lock == Lock::Free && self.step == Step::Idle && self.ptr == Which::Old {
            self.lock = Lock::Held;
            self.step = Step::Held;
            return true;
        }
        false
    }

    fn reserve(&mut self) -> bool {
        if self.lock == Lock::Held && self.step == Step::Held && self.cap_allows {
            self.step = Step::Reserved;
            return true;
        }
        false
    }

    fn refuse(&mut self) -> bool {
        if self.lock == Lock::Held && self.step == Step::Held && !self.cap_allows {
            self.lock = Lock::Free;
            self.step = Step::Refused;
            return true;
        }
        false
    }

    fn write_seg(&mut self) -> bool {
        if self.lock == Lock::Held && self.step == Step::Reserved && self.seg == Obj::Absent {
            self.step = Step::Staged;
            self.seg = Obj::Temp;
            return true;
        }
        false
    }

    fn validate_seg(&mut self) -> bool {
        if self.mutation != Mutation::PointerFirst
            && self.lock == Lock::Held
            && self.step == Step::Staged
            && self.seg == Obj::Temp
        {
            self.seg = Obj::Valid;
            return true;
        }
        false
    }

    fn write_man(&mut self) -> bool {
        if self.mutation != Mutation::PointerFirst
            && self.lock == Lock::Held
            && self.step == Step::Staged
            && self.seg == Obj::Valid
            && self.man == Obj::Absent
        {
            self.step = Step::Manifest;
            self.man = Obj::Temp;
            return true;
        }
        false
    }

    fn validate_man(&mut self) -> bool {
        if self.mutation != Mutation::PointerFirst
            && self.lock == Lock::Held
            && self.step == Step::Manifest
            && self.man == Obj::Temp
        {
            self.step = Step::Ready;
            self.man = Obj::Valid;
            return true;
        }
        false
    }

    fn swap(&mut self) -> bool {
        let ready = self.mutation != Mutation::PointerFirst
            && self.step == Step::Ready
            && self.seg == Obj::Valid
            && self.man == Obj::Valid;
        let early = self.mutation == Mutation::PointerFirst && self.step == Step::Staged && self.seg == Obj::Temp && self.man == Obj::Absent;
        if self.lock == Lock::Held && self.ptr == Which::Old && (ready || early) {
            self.ptr = Which::New;
            self.prev = Which::Old;
            self.pointed = true;
            self.step = Step::Swapped;
            return true;
        }
        false
    }

    fn release(&mut self) -> bool {
        if self.lock == Lock::Held && self.step == Step::Swapped {
            self.lock = Lock::Free;
            self.step = Step::Idle;
            return true;
        }
        false
    }

    fn crash(&mut self) -> bool {
        if self.lock == Lock::Held && self.crashes_left > 0 {
            self.lock = Lock::Dead;
            self.crashes_left -= 1;
            return true;
        }
        false
    }

    fn recover(&mut self) -> bool {
        if self.mutation == Mutation::StuckWriter || self.lock != Lock::Dead {
            return false;
        }
        self.lock = Lock::Free;
        self.step = Step::Idle;
        if self.ptr != Which::New {
            self.seg = Obj::Absent;
            self.man = Obj::Absent;
        }
        true
    }

    fn reader_begin(&mut self) -> bool {
        if self.lease_phase == Phase::Idle && self.acquires < 4 {
            self.lease_phase = Phase::Acquiring;
            self.observed = self.ptr;
            self.acquires += 1;
            self.lease = Which::None;
            return true;
        }
        false
    }

    fn confirm_old(&self) -> bool {
        self.observed == Which::Old
            && matches!(self.old_kept, Kept::Live | Kept::Retired)
            && (self.ptr == Which::Old || self.prev == Which::Old || self.old_kept == Kept::Live)
    }

    fn confirm_new(&self) -> bool {
        self.observed == Which::New && self.ptr == Which::New && self.seg == Obj::Valid && self.man == Obj::Valid
    }

    fn reader_confirm(&mut self) -> bool {
        if self.lease_phase == Phase::Acquiring && (self.confirm_old() || self.confirm_new()) {
            self.lease = self.observed;
            self.lease_phase = Phase::Active;
            return true;
        }
        false
    }

    fn reader_abort(&mut self) -> bool {
        if self.lease_phase == Phase::Acquiring && !self.confirm_old() && !self.confirm_new() {
            self.lease_phase = Phase::Idle;
            self.lease = Which::None;
            self.observed = Which::None;
            return true;
        }
        false
    }

    fn reader_release(&mut self) -> bool {
        if self.lease_phase == Phase::Active {
            self.lease_phase = Phase::Idle;
            self.lease = Which::None;
            self.observed = Which::None;
            return true;
        }
        false
    }

    fn drop_prev(&mut self) -> bool {
        if self.prev == Which::Old
            && self.ptr == Which::New
            && self.lock == Lock::Free
            && self.seg == Obj::Valid
            && self.man == Obj::Valid
            && self.step == Step::Idle
            && self.pointed
        {
            self.prev = Which::None;
            return true;
        }
        false
    }

    fn retire(&mut self) -> bool {
        let lease_blocks = self.mutation != Mutation::OmitLeases && self.lease_holds_old();
        if self.old_kept == Kept::Live && self.ptr != Which::Old && self.prev != Which::Old && !lease_blocks {
            self.old_kept = Kept::Retired;
            self.grace = 0;
            return true;
        }
        false
    }

    fn arm_grace(&mut self) -> bool {
        if self.old_kept == Kept::Retired && self.grace == 0 {
            self.grace = 1;
            return true;
        }
        false
    }

    fn reachable_old(&self) -> bool {
        self.ptr == Which::Old
            || self.prev == Which::Old
            || self.old_kept == Kept::Live
            || (self.mutation != Mutation::OmitLeases && self.lease_holds_old())
    }

    #[allow(clippy::wrong_self_convention)]
    fn to_pending(&mut self) -> bool {
        if self.old_kept == Kept::Retired && self.grace == 1 && !self.reachable_old() {
            self.old_kept = Kept::Pending;
            return true;
        }
        false
    }

    #[allow(clippy::wrong_self_convention)]
    fn to_gone(&mut self) -> bool {
        if self.old_kept == Kept::Pending && !self.reachable_old() {
            self.old_kept = Kept::Gone;
            return true;
        }
        false
    }

    fn apply(&mut self, action: u8) {
        let _ = match action % 20 {
            0 => self.acquire(),
            1 => self.reserve(),
            2 => self.refuse(),
            3 => self.write_seg(),
            4 => self.validate_seg(),
            5 => self.write_man(),
            6 => self.validate_man(),
            7 => self.swap(),
            8 => self.release(),
            9 => self.crash(),
            10 => self.recover(),
            11 => self.reader_begin(),
            12 => self.reader_confirm(),
            13 => self.reader_abort(),
            14 => self.reader_release(),
            15 => self.drop_prev(),
            16 => self.retire(),
            17 => self.arm_grace(),
            18 => self.to_pending(),
            _ => self.to_gone(),
        };
    }

    fn published(&self) -> bool {
        self.ptr == Which::New
            && self.pointed
            && self.lock == Lock::Free
            && self.step == Step::Idle
            && self.seg == Obj::Valid
            && self.man == Obj::Valid
    }

    fn fair(mutation: Mutation, cap_allows: bool, crash_once: bool) -> &'static str {
        let mut world = Self::new(mutation, cap_allows);
        let mut crashed = false;
        for _ in 0..40 {
            if world.lock == Lock::Dead {
                if !world.recover() {
                    return "stuck";
                }
                continue;
            }
            if world.step == Step::Refused {
                return "refused";
            }
            if world.published() {
                if world.old_kept == Kept::Gone {
                    return "published";
                }
                if world.lease_phase == Phase::Active {
                    world.reader_release();
                } else if !world.drop_prev() && !world.retire() && !world.arm_grace() && !world.to_pending() {
                    world.to_gone();
                }
                continue;
            }
            if crash_once && !crashed && world.step == Step::Staged && world.crash() {
                crashed = true;
                continue;
            }
            if world.acquire() || world.reserve() || world.refuse() || world.write_seg() || world.validate_seg() || world.write_man() || world.validate_man() || world.swap() || world.release() {
                continue;
            }
            return "halt";
        }
        "bound"
    }
}

#[cfg(test)]
mod tests;
