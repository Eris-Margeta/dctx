---------------------------- MODULE Lifecycle ----------------------------
\* Pre-exec platform gates and progress-based worker supervision (decision 077/080).
\* PLUGINS sections 3.1/3.4; PL-C S-ADMISSION and PP4/PP8.
\* Fair environment actions are explicit; PL-H/PL-A implement this machine.
EXTENDS Naturals
CONSTANTS Mutation, OsCanReap
VARIABLE s
vars == <<s>>
Origins == {"third-party", "bundled-git", "official-override", "unverified",
            "incompatible"}
Verified == s.origin # "unverified"
Compatible == s.origin # "incompatible"
Disposition == s.confined \/ s.origin = "bundled-git"
Init == \E origin \in Origins, confined \in BOOLEAN,
           activity \in {"useful", "heartbeat", "dead", "unknown"} :
        s = [origin |-> origin, confined |-> confined,
                 activity |-> activity, phase |-> "installed",
                 enabled |-> FALSE, executed |-> FALSE, negotiated |-> FALSE,
                 initialized |-> FALSE, admission |-> FALSE, halted |-> FALSE,
                 work |-> 0, dependency |-> "pending", observation |-> "running",
                 reason |-> "none", elapsed |-> "short", reapPending |-> FALSE]
CanEnable == Verified /\ Compatible /\
             (Disposition \/ Mutation = "unconfined-third-party" \/
              (Mutation = "override-exemption" /\ s.origin = "official-override"))
EnableOrRefuse ==
  /\ s.phase = "installed"
  /\ s' = [s EXCEPT !.phase = IF CanEnable THEN "enabled" ELSE "refused",
             !.enabled = CanEnable, !.reason = IF CanEnable THEN "none"
                                              ELSE "UnsupportedPlatform"]
Start == /\ s.phase = "enabled"
         /\ s' = [s EXCEPT !.phase = "spawned", !.executed = TRUE]
Negotiate == /\ s.phase = "spawned"
             /\ s' = [s EXCEPT !.phase = "negotiated", !.negotiated = TRUE]
Initialize == /\ s.phase = "negotiated"
              /\ s' = [s EXCEPT !.phase = "ready", !.initialized = TRUE]
Invoke == /\ s.phase = "ready" /\ ~s.halted
          /\ s' = [s EXCEPT !.phase = "running", !.admission = TRUE]
ResolveDependency ==
  /\ s.phase = "running" /\ s.dependency = "pending"
  /\ \E result \in {"resolved", "dead"} :
       s' = [s EXCEPT !.dependency = result]
Advance ==
  /\ s.phase = "running" /\ s.dependency = "resolved"
  /\ s.activity = "useful" /\ s.work < 2
  /\ s' = [s EXCEPT !.work = @ + 1, !.observation = "running"]
Finish == /\ s.phase = "running" /\ s.work = 2
          /\ s' = [s EXCEPT !.phase = "exited", !.reason = "completed",
                     !.admission = FALSE]
Observe == /\ s.phase = "running" /\ s.observation = "running"
           /\ s' = [s EXCEPT !.observation = "suspect"]
Probe == /\ s.phase = "running" /\ s.observation = "suspect"
         /\ s' = [s EXCEPT !.observation = "probe"]
Classify ==
  /\ s.phase = "running" /\ s.observation = "probe"
  /\ s' = [s EXCEPT !.observation =
       IF s.dependency = "pending" THEN "waiting"
       ELSE IF s.dependency = "dead" THEN "stalled"
       ELSE IF s.activity = "useful" THEN "running"
       ELSE IF Mutation = "heartbeat-progress" /\ s.activity = "heartbeat"
            THEN "running"
       ELSE "stalled"]
ObserveWait ==
  /\ s.phase = "running" /\ s.observation = "waiting"
  /\ s.dependency # "pending"
  /\ s' = [s EXCEPT !.observation = "suspect"]
CancelStalled ==
  /\ s.phase = "running" /\ s.observation = "stalled"
  /\ s' = [s EXCEPT !.phase = "cancelling", !.admission = FALSE,
             !.reason = IF s.activity = "unknown" THEN "UnresolvedSuspicion"
                        ELSE "Stalled"]
Halt ==
  /\ s.phase \in {"enabled", "spawned", "negotiated", "ready", "running"}
  /\ ~s.halted
  /\ s' = [s EXCEPT !.halted = TRUE, !.phase = "cancelling",
             !.admission = IF Mutation = "halt-keeps-admission" THEN @ ELSE FALSE,
             !.reason = "Cancelled"]
LateRequest ==
  /\ s.phase = "cancelling" /\ Mutation = "cancel-admission"
  /\ s' = [s EXCEPT !.admission = TRUE]
AcknowledgeOrForce ==
  /\ s.phase = "cancelling"
  /\ s' = [s EXCEPT !.phase = "exited", !.admission = FALSE]
Reap == /\ s.phase = "exited" /\ OsCanReap
        /\ s' = [s EXCEPT !.phase = "reaped", !.reapPending = FALSE]
ObserveReapPending == /\ s.phase = "exited" /\ ~OsCanReap
                      /\ s' = [s EXCEPT !.reapPending = TRUE]
ElapsedObservation ==
  /\ s.phase = "running" /\ s.elapsed = "short"
  /\ s' = [s EXCEPT !.elapsed = "long"]
TotalRuntimeKill ==
  /\ Mutation = "total-runtime-kill" /\ s.phase = "running"
  /\ s.activity = "useful" /\ s.elapsed = "long" /\ s.work > 0
  /\ s' = [s EXCEPT !.phase = "cancelling", !.admission = FALSE,
             !.reason = "RuntimeDeadline"]
Next == EnableOrRefuse \/ Start \/ Negotiate \/ Initialize \/ Invoke
        \/ ResolveDependency \/ Advance \/ Finish \/ Observe \/ Probe \/ Classify
        \/ ObserveWait \/ CancelStalled \/ Halt \/ LateRequest
        \/ AcknowledgeOrForce \/ Reap \/ ObserveReapPending
        \/ ElapsedObservation \/ TotalRuntimeKill
TypeOK == /\ s.work \in 0..2 /\ s.origin \in Origins
          /\ s.activity \in {"useful", "heartbeat", "dead", "unknown"}
PP4Confinement == /\ (s.enabled \/ s.executed => Verified /\ Compatible /\ Disposition)
                 /\ (s.admission => s.initialized /\ s.negotiated)
PP8Safety == /\ s.reason # "RuntimeDeadline"
             /\ (s.phase \in {"cancelling", "exited", "reaped"} => ~s.admission)
             /\ (s.halted => ~s.admission)
             /\ (~OsCanReap => s.phase # "reaped")
Terminal == s.phase \in {"refused", "reaped"}
PP8Progress == <>Terminal
Fairness == /\ WF_vars(EnableOrRefuse) /\ WF_vars(Start)
            /\ WF_vars(Negotiate) /\ WF_vars(Initialize) /\ WF_vars(Invoke)
            /\ WF_vars(ResolveDependency) /\ WF_vars(Advance) /\ WF_vars(Finish)
            /\ WF_vars(Observe) /\ WF_vars(Probe) /\ WF_vars(Classify)
            /\ WF_vars(ObserveWait) /\ WF_vars(CancelStalled)
            /\ WF_vars(AcknowledgeOrForce) /\ WF_vars(Reap)
NeverComplete == ~(s.phase = "reaped" /\ s.reason = "completed")
NeverBundled == ~(s.phase = "running" /\ s.origin = "bundled-git" /\ ~s.confined)
NeverThirdParty == ~(s.phase = "running" /\ s.origin = "third-party" /\ s.confined)
NeverSlowProgress == ~(s.work > 0 /\ s.elapsed = "long" /\ s.phase = "running")
NeverReapPending == ~s.reapPending
NeverStalledReaped == ~(s.phase = "reaped" /\ s.reason = "Stalled")
Spec == Init /\ [][Next]_vars /\ Fairness
SafetySpec == Init /\ [][Next]_vars
=============================================================================
