----------------------------- MODULE Isolation -----------------------------
\* Explicit facade composition and caller ceilings, with no core Git path.
\* PLUGINS sections 1/3.1/5.4; decision 078 and PL-C PP6.
\* Closure bits abstract build inputs; PL-F/PL-W/PL-V must audit real closures.
EXTENDS Naturals
CONSTANT Mutation
VARIABLE s
vars == <<s>>
Init == \E mode \in {"core", "strict", "default", "library"},
           operation \in {"pack", "map", "command"}, injected, enabled \in BOOLEAN :
        s = [mode |-> mode, operation |-> operation,
                 injected |-> injected, enabled |-> enabled,
                 host |-> FALSE, worker |-> FALSE, git |-> FALSE,
                 discovered |-> FALSE, engine |-> FALSE, coreGit |-> FALSE,
                 receipt |-> FALSE]
Strict == s.mode \in {"core", "strict"}
PluginFree == Strict \/ ~s.injected \/ ~s.enabled
CallerAllowsMutation == s.operation # "map"
Engine == /\ ~s.engine /\ s' = [s EXCEPT !.engine = TRUE]
Compose == /\ ~Strict /\ s.injected /\ ~s.host
           /\ s' = [s EXCEPT !.host = TRUE]
SpawnWorker == /\ s.host /\ s.enabled /\ ~s.worker
               /\ s' = [s EXCEPT !.worker = TRUE]
BrokerGit == /\ s.worker /\ ~s.git
             /\ (CallerAllowsMutation \/ Mutation = "map-elevation")
             /\ s' = [s EXCEPT !.git = TRUE, !.receipt = TRUE]
AmbientDiscovery == /\ Mutation = "ambient-discovery" /\ ~s.injected
                     /\ s' = [s EXCEPT !.discovered = TRUE, !.host = TRUE]
CoreGit == /\ Mutation = "core-git" /\ Strict
           /\ s' = [s EXCEPT !.coreGit = TRUE, !.git = TRUE]
Next == Engine \/ Compose \/ SpawnWorker \/ BrokerGit \/ AmbientDiscovery \/ CoreGit
TypeOK == s.mode \in {"core", "strict", "default", "library"}
PP6Isolation == /\ ~s.coreGit /\ ~s.discovered
                /\ (Strict => ~s.host /\ ~s.worker /\ ~s.git)
                /\ (PluginFree => ~s.worker /\ ~s.git)
                /\ (s.git => CallerAllowsMutation /\ s.receipt /\ s.worker)
NeverStrictResult == ~(Strict /\ s.engine)
NeverLibraryResult == ~(s.mode = "library" /\ ~s.injected /\ s.engine)
NeverPluginGit == ~s.git
NeverReadOnlyWorker == ~(s.operation = "map" /\ s.worker /\ ~s.git)
Spec == Init /\ [][Next]_vars
=============================================================================
