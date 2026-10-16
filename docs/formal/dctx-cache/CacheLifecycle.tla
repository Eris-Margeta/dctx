----------------------------- MODULE CacheLifecycle -----------------------------
\* DX-A2 cache lifecycle (ARCHITECTURE-PROPOSAL §4 and §7 row DX-A2).
\* Spec: CACHE-v1, SEGMENTS-v1 publication, decision 078 registered-root ownership.
\* One generation publish, one reader, one idle overlay. TLC checks P2–P4 and P8.
\* Depends on Integers. check.sh runs this module; no Rust crate calls it.

EXTENDS Integers

CONSTANTS
  Mutation,     \* "none" or a single falsifying mutation
  Cap,          \* global admission cap, in model units
  Soft,         \* LRU pressure watermark, in model units
  Unit,         \* one generation's worst-case segment+manifest bytes
  LegacyExtra,  \* pre-existing oversized bytes; 0 in the normal model
  UserPin,      \* 1 = explicit pin opts out of expiry and LRU
  MaxNow,       \* logical clock bound (TTL only; not a progress cut-off)
  Ttl,          \* idle-overlay expiry age in logical ticks
  MaxAcquires,  \* finite reader handshakes
  MaxCrashes    \* finite writer crashes

ASSUME Mutation \in {
  "none", "pointer-first", "omit-leases", "drop-reservation",
  "stuck-writer", "git-ownership"
}
ASSUME Cap \in Nat /\ Soft \in Nat /\ Unit \in Nat /\ Unit > 0
ASSUME Soft <= Cap /\ LegacyExtra \in Nat /\ UserPin \in {0, 1}
ASSUME MaxNow \in Nat /\ Ttl \in Nat /\ Ttl <= MaxNow /\ Ttl > 0
ASSUME MaxAcquires \in Nat /\ MaxAcquires > 0
ASSUME MaxCrashes \in Nat /\ MaxCrashes > 0

\* Registered dctx roots only. Git ids are not ownership, expiry, or reachability
\* inputs (decision 078; decision 077's git piggy-back is withdrawn). No action reads GitIds
\* except the git-ownership mutation, which is absent from pass configs.
RegisteredRoots == {"r1"}
GitIds == {"git-object"}
OverlayBytes == 1

VARIABLE s
vars == <<s>>

\* Buckets are disjoint. Moving a byte between them does not change the sum.
\* C counts the live or retired old generation, a swapped new generation, the
\* idle overlay, and any legacy extra. A temp generation is Sg until the pointer
\* swaps. R is the outstanding reservation. P is a failed or in-flight unlink.
Committed ==
  (IF s.oldKept \in {"live", "retired"} THEN Unit ELSE 0)
  + (IF s.ptr = "new" THEN Unit ELSE 0)
  + (IF s.overlay = "live" THEN OverlayBytes ELSE 0)
  + LegacyExtra

Staged ==
  IF s.ptr # "new" /\ (s.seg \in {"temp", "valid"} \/ s.man \in {"temp", "valid"})
  THEN Unit ELSE 0

Reserved ==
  IF s.step = "reserved" /\ s.seg = "absent"
  THEN Unit * (1 + s.extra) ELSE 0

Pending == IF s.oldKept = "pending" THEN Unit ELSE 0

Admitted == Committed + Staged + Reserved + Pending
LegacyCeiling == Unit + OverlayBytes + LegacyExtra

\* drop-reservation omits R from the admission check.
ByteSum(n) ==
  IF Mutation = "drop-reservation"
  THEN Committed + Staged + Pending + n
  ELSE Admitted + n

CanAdmit(n) ==
  /\ LegacyExtra = 0
  /\ ByteSum(n) <= Cap

LeaseHolds(g) == s.leasePhase = "active" /\ s.lease = g

\* omit-leases drops active leases from the mark set.
ReachableOld ==
  \/ s.ptr = "old"
  \/ s.prev = "old"
  \/ s.oldKept = "live"
  \/ (Mutation # "omit-leases" /\ LeaseHolds("old"))

Pressure == Admitted >= Soft
TtlDue == s.now - s.lastUse >= Ttl

ConfirmOld ==
  /\ s.observed = "old"
  /\ s.oldKept \in {"live", "retired"}
  /\ \/ s.ptr = "old"
     \/ s.prev = "old"
     \/ s.oldKept = "live"

ConfirmNew ==
  /\ s.observed = "new"
  /\ s.ptr = "new"
  /\ s.seg = "valid"
  /\ s.man = "valid"

Init ==
  s = [
    lock |-> "free",
    step |-> "idle",
    ptr |-> "old",
    prev |-> "none",
    pointed |-> FALSE,
    seg |-> "absent",
    man |-> "absent",
    extra |-> 0,
    oldKept |-> "live",
    grace |-> 0,
    overlay |-> "live",
    lease |-> "none",
    leasePhase |-> "idle",
    observed |-> "none",
    now |-> 0,
    lastUse |-> 0,
    clockFault |-> FALSE,
    acquires |-> 0,
    crashesLeft |-> MaxCrashes,
    gitRetained |-> FALSE,
    refusal |-> "none"
  ]

\* --- writer lock and publication -------------------------------------------
\* Crash points are the held steps: after the lock, after the reservation,
\* after the temp segment, after manifest write, after validation, after the
\* pointer swap, and before the lock release. Recovery never promotes a temp
\* object onto the pointer.

Acquire ==
  /\ s.lock = "free"
  /\ s.step = "idle"
  /\ s.ptr = "old"
  /\ s.refusal = "none"
  /\ s' = [s EXCEPT !.lock = "held", !.step = "held"]

Reserve ==
  /\ s.lock = "held"
  /\ s.step = "held"
  /\ CanAdmit(Unit)
  /\ s' = [s EXCEPT !.step = "reserved"]

Refuse ==
  /\ s.lock = "held"
  /\ s.step = "held"
  /\ ~CanAdmit(Unit)
  /\ s' = [s EXCEPT !.lock = "free", !.step = "refused", !.refusal = "CacheLimit"]

\* Second admission while the first reservation is still outstanding.
ExtraReserve ==
  /\ s.lock = "held"
  /\ s.step = "reserved"
  /\ s.seg = "absent"
  /\ s.extra = 0
  /\ CanAdmit(Unit)
  /\ s' = [s EXCEPT !.extra = 1]

WriteSeg ==
  /\ s.lock = "held"
  /\ s.step = "reserved"
  /\ s.seg = "absent"
  /\ s' = [s EXCEPT !.step = "staged", !.seg = "temp"]

ValidateSeg ==
  /\ Mutation # "pointer-first"
  /\ s.lock = "held"
  /\ s.step = "staged"
  /\ s.seg = "temp"
  /\ s' = [s EXCEPT !.seg = "valid"]

WriteMan ==
  /\ Mutation # "pointer-first"
  /\ s.lock = "held"
  /\ s.step = "staged"
  /\ s.seg = "valid"
  /\ s.man = "absent"
  /\ s' = [s EXCEPT !.step = "manifest", !.man = "temp"]

ValidateMan ==
  /\ Mutation # "pointer-first"
  /\ s.lock = "held"
  /\ s.step = "manifest"
  /\ s.man = "temp"
  /\ s' = [s EXCEPT !.step = "ready", !.man = "valid"]

\* Atomic pointer swap is the visibility linearization point.
ReadyToSwap ==
  /\ Mutation # "pointer-first"
  /\ s.step = "ready"
  /\ s.seg = "valid"
  /\ s.man = "valid"

\* pointer-first publishes the temp segment before validation.
EarlySwap ==
  /\ Mutation = "pointer-first"
  /\ s.step = "staged"
  /\ s.seg = "temp"
  /\ s.man = "absent"

Swap ==
  /\ s.lock = "held"
  /\ s.ptr = "old"
  /\ (ReadyToSwap \/ EarlySwap)
  /\ s' = [s EXCEPT !.ptr = "new", !.prev = "old", !.pointed = TRUE, !.step = "swapped"]

Release ==
  /\ s.lock = "held"
  /\ s.step = "swapped"
  /\ s' = [s EXCEPT !.lock = "free", !.step = "idle"]

Crash ==
  /\ s.lock = "held"
  /\ s.crashesLeft > 0
  /\ s' = [s EXCEPT !.lock = "dead", !.crashesLeft = s.crashesLeft - 1]

\* OS lock release, not PID age, proves the writer is gone. Unpublished temp
\* bytes are unowned and incomplete, so recovery reclaims them. A swapped
\* pointer stays. stuck-writer never takes this step.
Recover ==
  /\ Mutation # "stuck-writer"
  /\ s.lock = "dead"
  /\ IF s.ptr = "new"
     THEN s' = [s EXCEPT !.lock = "free", !.step = "idle", !.extra = 0]
     ELSE s' = [s EXCEPT !.lock = "free", !.step = "idle", !.extra = 0,
                          !.seg = "absent", !.man = "absent"]

\* --- reader pin handshake ---------------------------------------------------
\* Begin observes one pointer. Confirm registers the lease only if that
\* generation is still retained and complete. A handle then stays on that
\* generation until release.

ReaderBegin ==
  /\ s.leasePhase = "idle"
  /\ s.acquires < MaxAcquires
  /\ s' = [s EXCEPT !.leasePhase = "acquiring",
                     !.observed = s.ptr,
                     !.acquires = s.acquires + 1,
                     !.lease = "none"]

ReaderConfirm ==
  /\ s.leasePhase = "acquiring"
  /\ (ConfirmOld \/ ConfirmNew)
  /\ s' = [s EXCEPT !.leasePhase = "active", !.lease = s.observed]

ReaderAbort ==
  /\ s.leasePhase = "acquiring"
  /\ ~ConfirmOld
  /\ ~ConfirmNew
  /\ s' = [s EXCEPT !.leasePhase = "idle", !.lease = "none", !.observed = "none"]

ReaderRelease ==
  /\ s.leasePhase = "active"
  /\ s' = [s EXCEPT !.leasePhase = "idle", !.lease = "none", !.observed = "none"]

\* --- clock, LRU, TTL, reachability GC --------------------------------------
\* Tick is a logical clock for TTL. It is not a fairness assumption and it is
\* not what P8 uses to conclude progress. A backward step only records
\* clockFault; now does not decrease.

Tick ==
  /\ s.now < MaxNow
  /\ s' = [s EXCEPT !.now = s.now + 1]

Touch ==
  /\ s.lastUse < s.now
  /\ s' = [s EXCEPT !.lastUse = s.now]

BackwardClock ==
  /\ s.clockFault = FALSE
  /\ s' = [s EXCEPT !.clockFault = TRUE]

\* Previous pointer is a recovery reference until the new closure is validated.
DropPrev ==
  /\ s.prev = "old"
  /\ s.ptr = "new"
  /\ s.lock = "free"
  /\ s.seg = "valid"
  /\ s.man = "valid"
  /\ s.step = "idle"
  /\ s.pointed = TRUE
  /\ s' = [s EXCEPT !.prev = "none"]

\* LRU/TTL retire a manifest. They do not unlink its bytes.
Retire ==
  /\ s.oldKept = "live"
  /\ s.ptr # "old"
  /\ s.prev # "old"
  /\ UserPin = 0
  /\ (Mutation = "omit-leases" \/ ~LeaseHolds("old"))
  /\ (Pressure \/ TtlDue)
  /\ s' = [s EXCEPT !.oldKept = "retired", !.grace = 0]

\* One maintenance step stands for the grace period or an explicit GC bypass.
\* It is not a wall-clock deadline.
ArmGrace ==
  /\ s.oldKept = "retired"
  /\ s.grace = 0
  /\ s' = [s EXCEPT !.grace = 1]

ToPending ==
  /\ s.oldKept = "retired"
  /\ s.grace = 1
  /\ ~ReachableOld
  /\ s' = [s EXCEPT !.oldKept = "pending"]

ToGone ==
  /\ s.oldKept = "pending"
  /\ ~ReachableOld
  /\ s' = [s EXCEPT !.oldKept = "gone"]

\* Idle overlay: time expiry of an unpinned non-current object. Independent of
\* LRU pressure so TTL is exercisable without being P8's progress signal.
ExpireOverlay ==
  /\ s.overlay = "live"
  /\ UserPin = 0
  /\ TtlDue
  /\ s' = [s EXCEPT !.overlay = "gone"]

RetainByGit ==
  /\ Mutation = "git-ownership"
  /\ s.gitRetained = FALSE
  /\ s' = [s EXCEPT !.gitRetained = TRUE]

Next ==
  \/ Acquire \/ Reserve \/ Refuse \/ ExtraReserve
  \/ WriteSeg \/ ValidateSeg \/ WriteMan \/ ValidateMan \/ Swap \/ Release
  \/ Crash \/ Recover
  \/ ReaderBegin \/ ReaderConfirm \/ ReaderAbort \/ ReaderRelease
  \/ Tick \/ Touch \/ BackwardClock
  \/ DropPrev \/ Retire \/ ArmGrace \/ ToPending \/ ToGone
  \/ ExpireOverlay \/ RetainByGit

Spec == Init /\ [][Next]_vars

\* Each of these disjuncts is enabled alone at each protocol stage, so weak
\* fairness of the disjunction forces the next stage. Separate WF formulas
\* would say the same thing and cost more in the liveness tableau.
WriterAdvance ==
  Acquire \/ Reserve \/ Refuse \/ WriteSeg \/ ValidateSeg
  \/ WriteMan \/ ValidateMan \/ Swap \/ Release

Maintenance ==
  DropPrev \/ Retire \/ ArmGrace \/ ToPending \/ ToGone

\* Crash, Tick, Touch, BackwardClock, ReaderBegin and ExpireOverlay are not
\* fair on purpose:
\* - a crash may happen at most MaxCrashes times; progress must not assume it;
\* - the clock and TTL are not the signal that publication or recovery finishes;
\* - a reader may never arrive; a reader that holds a lease eventually releases;
\* - LRU under pressure reclaims a superseded generation without waiting for TTL.
Fairness ==
  /\ WF_vars(WriterAdvance)
  /\ WF_vars(Recover)
  /\ WF_vars(ReaderRelease)
  /\ WF_vars(Maintenance)

FairSpec == Spec /\ Fairness

TypeOK ==
  /\ s.lock \in {"free", "held", "dead"}
  /\ s.step \in {"idle", "held", "reserved", "staged", "manifest",
                 "ready", "swapped", "refused"}
  /\ s.ptr \in {"old", "new"}
  /\ s.prev \in {"none", "old"}
  /\ s.pointed \in BOOLEAN
  /\ s.seg \in {"absent", "temp", "valid"}
  /\ s.man \in {"absent", "temp", "valid"}
  /\ s.extra \in {0, 1}
  /\ s.oldKept \in {"live", "retired", "pending", "gone"}
  /\ s.grace \in {0, 1}
  /\ s.overlay \in {"live", "gone"}
  /\ s.lease \in {"none", "old", "new"}
  /\ s.leasePhase \in {"idle", "acquiring", "active"}
  /\ s.observed \in {"none", "old", "new"}
  /\ s.now \in 0..MaxNow
  /\ s.lastUse \in 0..MaxNow
  /\ s.lastUse <= s.now
  /\ s.clockFault \in BOOLEAN
  /\ s.acquires \in 0..MaxAcquires
  /\ s.crashesLeft \in 0..MaxCrashes
  /\ s.gitRetained \in BOOLEAN
  /\ s.refusal \in {"none", "CacheLimit"}
  /\ (s.lock = "free" => s.step \in {"idle", "refused"})
  /\ (s.lock = "held" => s.step \in {"held", "reserved", "staged", "manifest",
                                     "ready", "swapped"})
  /\ (s.lock = "dead" => s.step \in {"held", "reserved", "staged", "manifest",
                                     "ready", "swapped"})
  /\ (s.step = "refused" <=> s.refusal = "CacheLimit")
  /\ (s.leasePhase = "idle" => s.lease = "none" /\ s.observed = "none")
  /\ (s.leasePhase = "acquiring" => s.lease = "none" /\ s.observed \in {"old", "new"})
  /\ (s.leasePhase = "active" => s.lease = s.observed /\ s.lease \in {"old", "new"})
  /\ (s.step = "swapped" => s.ptr = "new" /\ s.pointed)

\* P2: a visible generation's objects are complete, and a handle names one.
P2 ==
  /\ (s.ptr = "new" <=> s.pointed)
  /\ (s.pointed => s.seg = "valid" /\ s.man = "valid")
  /\ (LeaseHolds("new") => s.pointed /\ s.seg = "valid" /\ s.man = "valid")
  /\ (s.leasePhase = "active" => s.lease \in {"old", "new"})

\* P3: a retained manifest, recovery pointer, published pointer, or active
\* lease still has its object. Unlink goes pending then gone; no partial size.
P3 ==
  /\ (LeaseHolds("old") => s.oldKept \in {"live", "retired"})
  /\ (s.ptr = "old" => s.oldKept # "gone")
  /\ (s.prev = "old" => s.oldKept \in {"live", "retired"})

\* P4: admission cap, or no growth while legacy state is already over the cap.
LegacyFrozen ==
  /\ Admitted <= LegacyCeiling
  /\ Staged = 0
  /\ Reserved = 0
  /\ s.ptr = "old"

P4 == IF LegacyExtra > 0 THEN LegacyFrozen ELSE Admitted <= Cap

\* Ownership inputs are registered roots, manifests, leases, TTL and metadata.
\* gitRetained is the mutation that treats a git id as a root.
NoGitOwnership ==
  /\ GitIds \intersect RegisteredRoots = {}
  /\ s.gitRetained = FALSE

\* An explicit pin blocks LRU and TTL retirement.
PinHolds ==
  UserPin = 0 \/ (s.oldKept = "live" /\ s.overlay = "live")

Published ==
  /\ s.ptr = "new"
  /\ s.pointed
  /\ s.lock = "free"
  /\ s.step = "idle"
  /\ s.seg = "valid"
  /\ s.man = "valid"

Refused ==
  /\ s.step = "refused"
  /\ s.lock = "free"
  /\ s.refusal = "CacheLimit"

\* P8 under FairSpec only. Completes or returns CacheLimit. A dead writer
\* loses the lock. After a successor is published, LRU plus lease release
\* reclaims the old generation unless an explicit pin blocks expiry.
\* Do not read a TLC time limit as this property; Tick is not in Fairness.
P8 ==
  /\ [](s.lock = "dead" => <> (s.lock = "free"))
  /\ <>(Published \/ Refused)
  /\ []( (Published /\ UserPin = 0) => <> (s.oldKept = "gone") )

\* Witnesses for expect-fail/. Pass configs do not assert these.
NoPublish == s.ptr = "old"
NoCrash == s.lock # "dead"
NoCollect == s.oldKept # "gone"
NoTtl == s.overlay = "live"

\* Mutations (expect-fail configs, not pass configs):
\* pointer-first    Swap before validation          falsifies P2
\* omit-leases      mark set ignores active leases  falsifies P3
\* drop-reservation admission ignores R             falsifies P4
\* stuck-writer     crash never releases the lock   falsifies P8
\* git-ownership    a git id becomes a retained root falsifies NoGitOwnership
=============================================================================
