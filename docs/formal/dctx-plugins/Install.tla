------------------------------ MODULE Install ------------------------------
\* Locked immutable installation, atomic activation, trust and worker leases.
\* PLUGINS section 5; decision 081 and PL-C S-ARTIFACT/distribution contract.
\* Verification is an abstract trusted port, not a cryptographic proof (PL-I/X).
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
VARIABLE s
vars == <<s>>
Versions == 0..2
Candidates == 1..2
NoVersion == 3
Checks == {"hash", "size", "signature", "trust", "target", "schema", "freshness"}
Phases == {"idle", "resolved", "downloaded", "verified", "staged", "installed",
           "done", "interrupted"}
Init == \E flaw \in {[v \in Candidates |-> "good"]} \cup
                    {[v \in Candidates |-> IF v = bad THEN c ELSE "good"] :
                      bad \in Candidates, c \in Checks} :
        s = [flaw |-> flaw, phase |-> [v \in Candidates |-> "idle"],
                 lock |-> 0, complete |-> {0}, verified |-> {0}, active |-> 0,
                 leases |-> {}, revoked |-> {}, highWater |-> 0,
                 launchToken |-> NoVersion, substituted |-> FALSE,
                 last |-> [kind |-> "none"], crashed |-> {}, executed |-> {},
                 activated |-> {}, rollbackApproved |-> FALSE]
Authentic(v) == IF v = 0 THEN TRUE ELSE s.flaw[v] = "good"
VerificationPasses(v) == \A c \in Checks : s.flaw[v] # c \/ Mutation = c
Allowed(v) == Authentic(v) /\ v \notin s.revoked
Acquire(v) == /\ s.lock = 0 /\ s.phase[v] = "idle"
              /\ s' = [s EXCEPT !.lock = v, !.phase[v] = "resolved",
                         !.last = [kind |-> "none"]]
Download(v) == /\ s.lock = v /\ s.phase[v] = "resolved"
               /\ s' = [s EXCEPT !.phase[v] = "downloaded"]
Verify(v) == /\ s.lock = v /\ s.phase[v] = "downloaded"
             /\ VerificationPasses(v)
             /\ s' = [s EXCEPT !.phase[v] = "verified", !.verified = @ \cup {v}]
Refuse(v) == /\ s.lock = v /\ s.phase[v] = "downloaded"
             /\ ~VerificationPasses(v)
             /\ s' = [s EXCEPT !.phase[v] = "done", !.lock = 0]
Stage(v) == /\ s.lock = v /\ s.phase[v] = "verified"
            /\ s' = [s EXCEPT !.phase[v] = "staged"]
Publish(v) == /\ s.lock = v /\ s.phase[v] = "staged"
              /\ s' = [s EXCEPT !.phase[v] = "installed", !.complete = @ \cup {v}]
Activate(v) ==
  /\ s.lock = v
  /\ (s.phase[v] = "installed" \/
       (Mutation = "activate-staging" /\ s.phase[v] = "staged"))
  /\ (v \notin s.revoked \/ Mutation = "activate-revoked")
  /\ (v >= s.highWater \/ Mutation = "implicit-downgrade")
  /\ s' = [s EXCEPT !.active = v, !.phase[v] = "done", !.lock = 0,
             !.activated = @ \cup {v},
             !.highWater = IF v > @ THEN v ELSE @,
             !.last = [kind |-> "activate", version |-> v,
                       previous |-> s.active, floor |-> s.highWater,
                       approved |-> FALSE]]
Crash(v) ==
  /\ s.lock = v
  /\ s' = [s EXCEPT !.lock = 0, !.crashed = @ \cup {s.phase[v]},
             !.phase[v] = "interrupted",
             !.active = IF Mutation = "crash-partial" THEN v ELSE @,
             !.last = [kind |-> "crash", previous |-> s.active]]
Recover(v) == /\ s.phase[v] = "interrupted" /\ s.lock = 0
              /\ s' = [s EXCEPT !.phase[v] = "done", !.last = [kind |-> "none"]]
Revoke(v) ==
  /\ v \notin s.revoked
  /\ s' = [s EXCEPT !.revoked = @ \cup {v},
             !.active = IF @ = v THEN NoVersion ELSE @,
             !.last = [kind |-> "none"]]
Revalidate ==
  /\ s.active # NoVersion /\ s.launchToken = NoVersion
  /\ Allowed(s.active) /\ s.active \in s.complete
  /\ s' = [s EXCEPT !.launchToken = s.active, !.substituted = FALSE]
Substitute == /\ s.launchToken # NoVersion /\ ~s.substituted
              /\ s' = [s EXCEPT !.substituted = TRUE]
Launch ==
  /\ s.launchToken # NoVersion /\ s.launchToken \in s.complete
  /\ Allowed(s.launchToken)
  /\ (~s.substituted \/ Mutation = "launch-identity")
  /\ s' = [s EXCEPT !.leases = @ \cup {s.launchToken},
             !.executed = @ \cup {s.launchToken}, !.launchToken = NoVersion,
             !.last = [kind |-> "launch", version |-> s.launchToken,
                       actual |-> IF s.substituted THEN 4 ELSE s.launchToken]]
RefuseLaunch ==
  /\ s.launchToken # NoVersion
  /\ (s.substituted \/ ~Allowed(s.launchToken) \/ s.launchToken \notin s.complete)
  /\ s' = [s EXCEPT !.launchToken = NoVersion, !.last = [kind |-> "none"]]
EndWorker(v) == /\ v \in s.leases
                /\ s' = [s EXCEPT !.leases = @ \ {v}, !.last = [kind |-> "none"]]
Remove(v) ==
  /\ v \in s.complete /\ v # s.active
  /\ (v \notin s.leases \/ Mutation = "unlink-lease")
  /\ IF v = 0 THEN TRUE ELSE s.phase[v] \in {"done", "interrupted"}
  /\ s' = [s EXCEPT !.complete = @ \ {v}, !.last = [kind |-> "none"]]
ApproveRollback == /\ ~s.rollbackApproved
                   /\ s' = [s EXCEPT !.rollbackApproved = TRUE]
Rollback == /\ s.rollbackApproved /\ 0 \in s.complete /\ Allowed(0)
            /\ s' = [s EXCEPT !.active = 0,
                       !.last = [kind |-> "activate", version |-> 0,
                                 previous |-> s.active, floor |-> s.highWater,
                                 approved |-> TRUE]]
Next == (\E v \in Candidates : Acquire(v) \/ Download(v) \/ Verify(v)
          \/ Refuse(v) \/ Stage(v) \/ Publish(v) \/ Activate(v) \/ Crash(v) \/ Recover(v))
        \/ (\E v \in Versions : Revoke(v) \/ EndWorker(v) \/ Remove(v))
        \/ Revalidate \/ Substitute \/ Launch \/ RefuseLaunch
        \/ ApproveRollback \/ Rollback
TypeOK == /\ s.active \in 0..3 /\ s.launchToken \in 0..3 /\ s.lock \in 0..2
          /\ s.phase \in [Candidates -> Phases] /\ s.complete \subseteq Versions
          /\ s.leases \subseteq Versions /\ s.highWater \in Versions
PP9Integrity ==
  /\ (s.active # NoVersion => s.active \in s.complete /\
                           s.active \in s.verified /\ Allowed(s.active))
  /\ s.leases \subseteq s.complete
  /\ (s.last.kind = "crash" => s.active = s.last.previous)
  /\ (s.last.kind = "activate" =>
       s.last.version >= s.last.floor \/ s.last.approved)
  /\ (s.last.kind = "launch" => s.last.actual = s.last.version)
NeverNewActive == ~(s.active \in Candidates)
NeverUpdatedLease == ~\E v \in s.leases : s.active # NoVersion /\ v # s.active
NeverLaunch == s.executed = {}
NeverConcurrentUpdates == s.activated # Candidates
NeverCrashResolved == "resolved" \notin s.crashed
NeverCrashDownloaded == "downloaded" \notin s.crashed
NeverCrashVerified == "verified" \notin s.crashed
NeverCrashStaged == "staged" \notin s.crashed
NeverCrashInstalled == "installed" \notin s.crashed
NeverRollback == ~(s.last.kind = "activate" /\ s.last.floor > 0 /\ s.last.approved)
Spec == Init /\ [][Next]_vars
=============================================================================
