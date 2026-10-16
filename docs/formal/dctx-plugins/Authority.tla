---------------------------- MODULE Authority ----------------------------
\* Broker grants, source provenance and opaque protocol bindings.
\* PLUGINS sections 3.1, 3.3 and 4; PL-C S-GRANT/S-PRIVACY/S-STATE.
\* Finite host decisions feed this model; PL-H/PL-A refine its actions.
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
VARIABLE s
vars == <<s>>
Caps == {"read", "write"}
Decisions == {"Include", "Exclude", "OmitContent", "DebugVisible"}
Context == [plugin |-> 1, artifact |-> 1, root |-> 1, invocation |-> 1,
            session |-> 1, generation |-> 1, namespace |-> 1]
Tokens == {Context} \cup
          {[Context EXCEPT ![f] = 2] : f \in DOMAIN Context}
Effective == s.manifest \cap s.consent \cap s.org \cap s.host \cap
             s.selection \cap s.available
AdmissionGrant == IF Mutation = "omit-org"
                  THEN s.manifest \cap s.consent \cap s.host \cap
                       s.selection \cap s.available ELSE Effective
Bound(t) == t = Context
CheckBinding(t) == CASE Mutation = "cross-root" -> t = [Context EXCEPT !.root = t.root]
                    [] Mutation = "cross-artifact" -> t = [Context EXCEPT !.artifact = t.artifact]
                    [] Mutation = "cross-invocation" -> t = [Context EXCEPT !.invocation = t.invocation]
                    [] Mutation = "cross-session" -> t = [Context EXCEPT !.session = t.session]
                    [] Mutation = "cross-generation" -> t = [Context EXCEPT !.generation = t.generation]
                    [] Mutation = "cross-plugin" -> t = [Context EXCEPT !.plugin = t.plugin]
                    [] OTHER -> Bound(t)
Event(kind, cap, token, epoch) ==
  [kind |-> kind, cap |-> cap, token |-> token, epoch |-> epoch,
   currentEpoch |-> s.epoch, authority |-> Effective, halted |-> s.halted,
   source |-> s.source, provenance |-> s.provenance, ready |-> s.ready]
None == [kind |-> "none"]
Init == s = [manifest |-> Caps, consent |-> Caps, approved |-> Caps, org |-> {"read"},
             host |-> Caps, selection |-> Caps, available |-> Caps,
             epoch |-> 0, halted |-> FALSE, ready |-> FALSE,
             source |-> "Include", provenance |-> "Include",
             pending |-> [cap |-> "none", epoch |-> 0], event |-> None,
             consentChanged |-> FALSE, configured |-> FALSE,
             committed |-> FALSE, readDone |-> FALSE, adminDone |-> FALSE]
Initialize == /\ ~s.ready /\ ~s.halted
              /\ s' = [s EXCEPT !.ready = TRUE, !.event = None]
Configure(c) == /\ ~s.configured /\ c \subseteq Caps
                /\ s' = [s EXCEPT !.manifest = c, !.configured = TRUE,
                           !.consent = IF Mutation = "config-consent"
                                       THEN Caps ELSE @,
                           !.event = None]
Consent == /\ ~s.consentChanged
           /\ s' = [s EXCEPT !.org = Caps, !.consentChanged = TRUE,
                      !.event = None]
Narrow == /\ s.epoch = 0
          /\ s' = [s EXCEPT !.consent = {"read"}, !.approved = {"read"}, !.selection = {"read"},
                     !.epoch = 1, !.event = None]
ChangeSelection(d, p) == /\ s.epoch = 0 /\ d \in Decisions /\ p \in Decisions
                         /\ s' = [s EXCEPT !.source = d, !.provenance = p,
                                    !.epoch = 1, !.event = None]
Halt == /\ ~s.halted
        /\ s' = [s EXCEPT !.halted = TRUE, !.epoch = 1, !.event = None]
Admit(c, t) ==
  /\ s.pending.cap = "none" /\ ~s.committed
  /\ c \in AdmissionGrant /\ CheckBinding(t)
  /\ (s.ready \/ Mutation = "before-ready")
  /\ (~s.halted \/ Mutation = "after-halt")
  /\ s' = [s EXCEPT !.pending = [cap |-> c, epoch |-> s.epoch],
             !.event = Event("admit", c, t, s.epoch)]
Commit ==
  /\ s.pending.cap # "none" /\ ~s.committed /\ ~s.halted
  /\ (Mutation = "stale-commit" \/
       (s.pending.cap \in Effective /\ s.pending.epoch = s.epoch))
  /\ s' = [s EXCEPT !.committed = TRUE,
             !.event = Event("commit", s.pending.cap, Context, s.pending.epoch)]
Read(kind, t) ==
  /\ ~s.readDone /\ ~s.halted /\ s.ready /\ "read" \in Effective
  /\ (t.namespace = Context.namespace \/ Mutation = "state-namespace")
  /\ IF kind \in {"cached", "state"} THEN
       (s.source = "Include" /\
        (s.provenance = "Include" \/ Mutation = "stale-cache"))
     ELSE IF Mutation = "hash-excluded" /\ kind = "hash" THEN TRUE
     ELSE s.source = "Include" \/
          (Mutation = "debug-content" /\ s.source = "DebugVisible") \/
          (Mutation = "omit-content" /\ s.source = "OmitContent")
  /\ s' = [s EXCEPT !.readDone = TRUE,
             !.event = Event(kind, "read", t, s.epoch)]
Admin == /\ ~s.adminDone /\ ~s.halted /\ s.ready
         /\ s' = [s EXCEPT !.adminDone = TRUE,
                    !.event = Event(IF Mutation = "admin-history"
                                   THEN "history" ELSE "admin", "read",
                                   Context, s.epoch)]
Next == Initialize \/ Consent \/ Narrow \/ Halt \/ Commit \/ Admin
        \/ (\E c \in SUBSET Caps : Configure(c))
        \/ (\E d, p \in Decisions : ChangeSelection(d, p))
        \/ (\E c \in Caps, t \in Tokens : Admit(c, t))
        \/ (\E k \in {"read", "hash", "delivery", "cached", "state"},
               t \in {Context, [Context EXCEPT !.namespace = 2]} : Read(k, t))
TypeOK == /\ s.epoch \in 0..1 /\ s.consent \subseteq Caps
          /\ s.source \in Decisions /\ s.provenance \in Decisions
          /\ s.pending.cap \in Caps \cup {"none"}
PP1Authority ==
  /\ s.consent \subseteq s.approved
  /\ IF s.event.kind \in {"admit", "commit"} THEN
       /\ s.event.cap \in s.event.authority
       /\ s.event.epoch = s.event.currentEpoch /\ ~s.event.halted
     ELSE TRUE
PP2Privacy == IF s.event.kind \in {"read", "hash", "delivery", "cached", "state"}
              THEN /\ s.event.source = "Include"
                   /\ s.event.token.namespace = Context.namespace
                   /\ (s.event.kind \in {"cached", "state"} => s.event.provenance = "Include")
              ELSE s.event.kind # "history"
PP4Binding == IF s.event.kind \in {"admit", "commit"}
              THEN s.event.ready /\ Bound(s.event.token) ELSE TRUE
NeverCommit == ~s.committed
NeverRead == ~s.readDone
NeverAdmin == ~s.adminDone
NeverNewConsentWrite == ~(s.event.kind = "commit" /\ s.pending.cap = "write")
Spec == Init /\ [][Next]_vars
=============================================================================
