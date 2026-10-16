---------------------------- MODULE Resources ----------------------------
\* Quota accounting before allocation, including aggregate and wire sizes.
\* PLUGINS sections 3.2/3.3; PL-C S-BOUNDS and PP7.
\* Each dimension is checked independently; PL-P/PL-H refine checked sums.
EXTENDS Naturals
CONSTANT Mutation
VARIABLE s
vars == <<s>>
Dimensions == {"frame", "queue", "content", "diagnostics", "state", "actions",
               "upload", "memory", "work"}
Init == \E dimension \in Dimensions :
        s = [dimension |-> dimension, used |-> 0, accounted |-> 0,
                 peak |-> 0, refused |-> FALSE, accepted |-> FALSE,
                 raw |-> 0, wire |-> 0]
HostLimit == 3
OrganizationLimit == 2
UserLimit == 3
ManifestLimit == 3
Limit == 2
Wire(n) == IF s.dimension = "frame" THEN n + 1 ELSE n
Receive(n) ==
  LET charge == Wire(n)
      sum == s.used + charge
      checked == CASE Mutation = "per-chunk" -> charge
                   [] Mutation = "wrap" -> sum % 4
                   [] Mutation = "omit-wire-overhead" -> s.used + n
                   [] OTHER -> sum
      ceiling == IF Mutation = "omit-quota-minimum" THEN HostLimit ELSE Limit
  IN /\ n \in 1..3 /\ s.used <= Limit
     /\ IF checked <= ceiling THEN
          s' = [s EXCEPT !.used = sum, !.accounted = checked, !.peak = sum,
                     !.raw = n, !.wire = charge, !.accepted = TRUE]
        ELSE s' = [s EXCEPT !.refused = TRUE,
                     !.peak = IF Mutation = "allocate-before-check" THEN sum ELSE @]
Drain == /\ s.used > 0 /\ s.used <= Limit
         /\ s' = [s EXCEPT !.used = 0, !.accounted = 0]
Next == (\E n \in 1..3 : Receive(n)) \/ Drain
TypeOK == /\ s.dimension \in Dimensions /\ s.used \in 0..6
          /\ s.accounted \in 0..6 /\ s.peak \in 0..6
PP7Bounds == /\ s.used <= Limit /\ s.peak <= Limit /\ s.accounted = s.used
             /\ Limit <= HostLimit /\ Limit <= OrganizationLimit
             /\ Limit <= UserLimit /\ Limit <= ManifestLimit
NeverExactLimit == ~(s.accepted /\ s.used = Limit)
NeverRefusal == ~s.refused
Spec == Init /\ [][Next]_vars
=============================================================================
