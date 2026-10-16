----------------------------- MODULE Receipts -----------------------------
\* Durable intent, external execution, recovery and idempotent broker keys.
\* PLUGINS sections 3.3 and 4.4; PL-C S-AUDIT/S-REPLAY/S-RESULT.
\* Two ledger slots preserve competing keys; PL-H refines these actions.
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
VARIABLE s
vars == <<s>>
BaseKey == [artifact |-> 1, root |-> 1, invocation |-> 1, action |-> 1]
OtherKeys == {[BaseKey EXCEPT ![f] = 2] : f \in DOMAIN BaseKey}
Args == {"a", "b"}
Empty == [status |-> "absent", arg |-> "none", intent |-> FALSE,
          archived |-> FALSE, executions |-> 0]
Init == \E key2 \in OtherKeys :
        s = [key2 |-> key2, ledger |-> [i \in 1..2 |-> Empty],
                 audit |-> TRUE, failed |-> FALSE, last |-> [kind |-> "none"],
                 effect |-> [seen |-> FALSE], completed |-> FALSE,
                 recovered |-> FALSE, deduplicated |-> FALSE]
Key(i) == IF i = 1 THEN BaseKey ELSE s.key2
Slot(i) == IF Mutation = "key-collision" THEN 1 ELSE i
Receipt(kind, i, arg, slot) ==
  [kind |-> kind, key |-> Key(i), boundKey |-> Key(slot), arg |-> arg,
   boundArg |-> s.ledger[slot].arg, audit |-> s.audit]
Request(i, arg) ==
  LET j == Slot(i) IN
  /\ s.ledger[j].status = "absent"
  /\ (s.audit \/ Mutation = "audit-full")
  /\ s' = [s EXCEPT
             !.ledger[j] = [Empty EXCEPT !.status = "pending", !.arg = arg,
                             !.intent = Mutation # "effect-before-intent"],
             !.last = Receipt("admit", i, arg, j)]
Execute(i) ==
  /\ s.ledger[i].status = "pending" /\ s.audit
  /\ s' = [s EXCEPT !.ledger[i].status = "executed",
             !.ledger[i].executions = @ + 1,
             !.effect = [seen |-> TRUE, intent |-> s.ledger[i].intent,
                         audit |-> s.audit], !.last = [kind |-> "none"]]
Complete(i) ==
  /\ s.ledger[i].status = "executed" /\ s.audit
  /\ s' = [s EXCEPT !.ledger[i].status = "committed", !.completed = TRUE,
             !.last = Receipt("committed", i, s.ledger[i].arg, i)]
AuditFailure ==
  /\ ~s.failed
  /\ s' = [s EXCEPT !.audit = FALSE, !.failed = TRUE,
             !.last = [kind |-> "none"]]
RestoreAudit == /\ s.failed /\ ~s.audit
                /\ s' = [s EXCEPT !.audit = TRUE, !.last = [kind |-> "none"]]
CrashOrUncertain(i, outcome) ==
  /\ s.ledger[i].status \in {"pending", "executed"}
  /\ outcome \in {"unknown", "partial"}
  /\ s' = [s EXCEPT !.ledger[i].status = outcome, !.recovered = TRUE,
             !.ledger[i].intent = IF Mutation = "lose-intent" THEN FALSE ELSE @,
             !.last = Receipt("uncertain", i, s.ledger[i].arg, i)]
Repeat(i, arg) ==
  LET j == Slot(i) IN
  /\ s.ledger[j].status # "absent"
  /\ s' = [s EXCEPT
       !.last = Receipt(IF arg # s.ledger[j].arg /\ Mutation # "args-conflict"
                        THEN "conflict" ELSE "existing", i, arg, j),
       !.deduplicated = @ \/ (s.ledger[j].status = "committed" /\
                             arg = s.ledger[j].arg)]
BlindReplay(i) ==
  /\ (Mutation = "repeat-committed" /\ s.ledger[i].status = "committed")
      \/ (Mutation = "retry-unknown" /\ s.ledger[i].status = "unknown")
      \/ (Mutation = "retry-partial" /\ s.ledger[i].status = "partial")
      \/ (Mutation = "retry-pending" /\ s.ledger[i].status = "executed")
  /\ s.ledger[i].executions < 2
  /\ s' = [s EXCEPT !.ledger[i].status = "pending",
             !.last = Receipt("retry", i, s.ledger[i].arg, i)]
Reconcile(i) ==
  /\ s.ledger[i].status \in {"unknown", "partial"}
  /\ s.ledger[i].executions = 1 /\ s.audit
  /\ s' = [s EXCEPT !.ledger[i].status = "committed", !.completed = TRUE,
             !.last = Receipt("reconciled", i, s.ledger[i].arg, i)]
Rotate(i) ==
  /\ s.ledger[i].intent /\ ~s.ledger[i].archived
  /\ (s.ledger[i].status = "committed" \/ Mutation = "rotate-pending")
  /\ s' = [s EXCEPT !.ledger[i].intent = FALSE, !.ledger[i].archived = TRUE,
             !.last = [kind |-> "none"]]
Next == AuditFailure \/ RestoreAudit
        \/ (\E i \in 1..2, arg \in Args : Request(i, arg) \/ Repeat(i, arg))
        \/ (\E i \in 1..2 : Execute(i) \/ Complete(i) \/ BlindReplay(i)
                            \/ Reconcile(i) \/ Rotate(i))
        \/ (\E i \in 1..2, o \in {"unknown", "partial"} : CrashOrUncertain(i, o))
TypeOK == /\ s.key2 \in OtherKeys
          /\ \A i \in 1..2 : /\ s.ledger[i].executions \in 0..2
                              /\ s.ledger[i].arg \in Args \cup {"none"}
PP3Audit ==
  /\ (s.effect.seen => s.effect.intent /\ s.effect.audit)
  /\ (s.last.kind = "admit" => s.last.audit)
  /\ \A i \in 1..2 :
       /\ (s.ledger[i].status \in {"executed", "committed", "unknown", "partial"}
           => s.ledger[i].intent \/ s.ledger[i].archived)
       /\ (s.ledger[i].status \in {"pending", "executed", "unknown", "partial"}
           => ~s.ledger[i].archived)
PP5Replay ==
  /\ \A i \in 1..2 : s.ledger[i].executions <= 1
  /\ IF s.last.kind = "existing" THEN
       s.last.key = s.last.boundKey /\ s.last.arg = s.last.boundArg
     ELSE s.last.kind # "retry"
NeverComplete == ~s.completed
NeverUnknownAfterEffect == ~\E i \in 1..2 :
  s.ledger[i].status = "unknown" /\ s.ledger[i].executions = 1
NeverDedupe == ~s.deduplicated
NeverTwoKeys == ~(s.ledger[1].executions = 1 /\ s.ledger[2].executions = 1)
NeverReconciled == ~(s.completed /\ s.recovered)
Spec == Init /\ [][Next]_vars
=============================================================================
