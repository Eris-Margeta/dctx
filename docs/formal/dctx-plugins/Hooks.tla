------------------------------- MODULE Hooks -------------------------------
\* Canonical acyclic hook planning, predecessor refusal and committed results.
\* PLUGINS sections 3.1/3.5; PL-C S-RESULT and PP10.
\* Receipts are supplied by the broker model; PL-D/PL-H/PL-F refine scheduling.
EXTENDS Naturals, Sequences, FiniteSets
CONSTANT Mutation
VARIABLE s
vars == <<s>>
Plugins == {1, 2}
Orders == {<<1, 2>>, <<2, 1>>}
Position(order, h) == CHOOSE i \in 1..2 : order[i] = h
Legal(order) == \A edge \in s.edges : Position(order, edge[1]) < Position(order, edge[2])
ValidOrders == {o \in Orders : Legal(o)}
Canonical == IF <<1, 2>> \in ValidOrders THEN <<1, 2>> ELSE <<2, 1>>
Init == \E edges \in SUBSET (Plugins \X Plugins), required \in SUBSET Plugins,
           boundary \in {"pre", "post"} :
        s = [edges |-> edges, required |-> required, boundary |-> boundary,
                 phase |-> "plan", order |-> <<>>, done |-> 0,
                 status |-> [h \in Plugins |-> "pending"],
                 effects |-> {}, output |-> FALSE, published |-> FALSE,
                 reported |-> FALSE, report |-> {}, claims |-> {}, reportOutput |-> FALSE,
                 ranBlocked |-> FALSE]
Plan ==
  /\ s.phase = "plan"
  /\ IF ValidOrders = {} /\ Mutation # "ignore-cycle" THEN
       s' = [s EXCEPT !.phase = "refused"]
     ELSE s' = [s EXCEPT !.phase = "hooks",
                  !.order = IF Mutation = "ignore-cycle" THEN <<1, 2>>
                            ELSE IF Mutation = "ignore-tie" /\ Cardinality(ValidOrders) = 2
                                 THEN <<2, 1>> ELSE Canonical]
Blocked(h) == \E p \in Plugins : <<p, h>> \in s.edges /\
                                s.status[p] \in {"required-failure", "skipped"}
RunHook(outcome, effect) ==
  /\ s.phase = "hooks" /\ s.done < 2
  /\ (s.boundary = "pre" \/ s.published)
  /\ LET h == s.order[s.done + 1]
         blocked == Blocked(h)
         runs == ~blocked \/ Mutation = "run-dependent"
         result == IF ~runs THEN "skipped"
                   ELSE IF outcome = "ok" THEN "ok"
                   ELSE IF h \in s.required THEN "required-failure"
                   ELSE "advisory-failure"
     IN s' = [s EXCEPT !.done = @ + 1, !.status[h] = result,
                        !.ranBlocked = @ \/ (runs /\ blocked),
                        !.effects = IF runs /\ effect THEN @ \cup {h} ELSE @,
                        !.output = IF Mutation = "post-rollback" /\
                                      s.published /\ outcome = "failure"
                                   THEN FALSE ELSE @]
Publish ==
  /\ s.phase = "hooks" /\ ~s.published
  /\ (s.boundary = "post" \/ (s.done = 2 /\
       \A h \in s.required : s.status[h] # "required-failure"))
  /\ s' = [s EXCEPT !.published = TRUE, !.output = TRUE]
Report(claims) ==
  /\ s.done = 2 /\ ~s.reported
  /\ s.published \/ (s.boundary = "pre" /\
                      \E h \in s.required : s.status[h] = "required-failure")
  /\ s' = [s EXCEPT !.reported = TRUE, !.claims = claims,
             !.reportOutput = s.output,
             !.report = IF Mutation = "forged-receipt" THEN claims ELSE s.effects]
Next == Plan \/ Publish
        \/ (\E o \in {"ok", "failure"}, e \in BOOLEAN : RunHook(o, e))
        \/ (\E c \in SUBSET {1, 2, 3} : Report(c))
TypeOK == /\ s.done \in 0..2 /\ s.effects \subseteq Plugins
          /\ s.order \in Orders \cup {<<>>}
PP10OrderTruth ==
  /\ (s.phase = "hooks" => ValidOrders # {} /\ s.order = Canonical /\ Legal(s.order))
  /\ ~s.ranBlocked
  /\ (s.published => s.output)
  /\ (s.reported => s.report = s.effects /\ s.reportOutput = s.output)
  /\ (s.published /\ s.boundary = "pre" =>
       \A h \in s.required : s.status[h] # "required-failure")
NeverPublished == ~s.published
NeverPostFailure == ~(s.published /\ s.boundary = "post" /\
                     \E h \in Plugins : s.status[h] = "required-failure")
NeverSkip == ~\E h \in Plugins : s.status[h] = "skipped"
NeverReportedEffect == ~(s.reported /\ s.report # {})
NeverRejectedForgery == ~(s.reported /\ 3 \in s.claims /\ 3 \notin s.report)
NeverCycleRefused == s.phase # "refused"
Spec == Init /\ [][Next]_vars
=============================================================================
