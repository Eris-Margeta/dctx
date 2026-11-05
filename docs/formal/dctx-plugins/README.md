# Plugin state machines and TLC gate

PL-M implements the formal-model row in [PLUGINS §7.1](../../design/PLUGINS.md)
and the frozen [PL-C properties](../../design/plugin-api/properties.md).
These finite TLA+ models describe the host contracts; they do not establish
that the Rust implementation, a cryptographic primitive or an OS provider
refines those contracts. No product runtime code or dependencies change here.
The completed remote run is recorded in [VALIDATION.md](VALIDATION.md).

Run the complete gate through the authorized VM runner, from the worktree root:

```sh
test -s docs/formal/dctx-plugins/check.sh
sh docs/formal/dctx-plugins/check.sh
```

`check.sh` uses an existing
`tla2tools.jar` at the VM's standard installation locations, or the absolute
`TLA2TOOLS_JAR` environment setting. It downloads nothing. Java, GNU timeout
and sha256sum are VM prerequisites. `TLC_TIMEOUT` defaults to 300 seconds per
configuration, `TLC_WORKERS` to two, and Java's heap ceiling is 1536 MiB.
The checker timeout is an infrastructure limit, **not** a worker liveness rule.

The closed [suite manifest](suite.tsv) runs eight baseline configurations,
59 deliberately faulty configurations and 37 reachability witnesses. Baselines
must exhaust their finite state graph, exit zero and report no error. A safety
mutant must exit 12 and name its exact expected violated invariant in a trace
with at least two states. The liveness mutant must exit 13 and report a temporal
counterexample cycle. Parse errors, type errors, wrong-property violations,
timeouts, resource kills and surviving mutants fail the gate. No simulation,
depth cut-off, state constraint, symmetry reduction or probabilistic sampling
stands in for exhaustive checking. Deadlock checking is disabled because
refused/completed states intentionally admit only TLA+ stuttering; PP8 uses
an explicit temporal property instead.

Each invocation retains full TLC stdout/stderr, counterexamples, state metadata,
per-case results, tool/source SHA-256 digests and Java/settings records under
`.runs/run.*` **inside this owned directory on the VM**. The runner prints that
path and each baseline's state count. A failed
run stops at the first unexpected outcome. An unchanged rerun is not part of
this workflow; the kernel can independently execute the acceptance predicate.

## Properties and model boundaries

| Property | Checked predicates and model | Successful-path witnesses |
|---|---|---|
| PP1 authority | `Authority.PP1Authority`: six-way capability intersection at admission and commit, epoch, halt, and consent ceiling | `NeverCommit`, `NeverNewConsentWrite` |
| PP2 privacy | `Authority.PP2Privacy`: Include before read/hash/delivery, current derived/state provenance, namespace, no administrative history delivery | `NeverRead`, `NeverAdmin` |
| PP3 audit | `Receipts.PP3Audit`: intent precedes execution, audit capacity gates admission/execution, linked unresolved intents stay pinned | `NeverComplete`, `NeverUnknownAfterEffect`, `NeverReconciled` |
| PP4 binding | `Authority.PP4Binding` and `Lifecycle.PP4Confinement`: opaque bindings, readiness, host-established executable/platform disposition | `Authority.NeverCommit`, `Lifecycle.NeverThirdParty`, `NeverBundled` |
| PP5 replay | `Receipts.PP5Replay`: full key/arguments, at most one execution in the retained ledger, no re-admission of uncertain work | `NeverDedupe`, `NeverTwoKeys` |
| PP6 isolation | `Isolation.PP6Isolation`: core/strict closure, explicit host injection, no ambient discovery, inherited read-only map ceiling | `NeverStrictResult`, `NeverLibraryResult`, `NeverPluginGit`, `NeverReadOnlyWorker` |
| PP7 bounds | `Resources.PP7Bounds`: minimum ceiling, checked aggregate sum, wire overhead, allocation before admission, release | `NeverExactLimit`, `NeverRefusal` |
| PP8 progress | `Lifecycle.PP8Progress` and `PP8Safety`: eventual refusal/reaping, immediate halt, no total-runtime kill | `NeverComplete`, `NeverSlowProgress`, `NeverStalledReaped`, `NeverReapPending` |
| PP9 installation | `Install.PP9Integrity`: all verification gates, atomic registry switch/recovery, monotonic update, explicit rollback, launch identity, leases | `NeverNewActive`, `NeverLaunch`, `NeverConcurrentUpdates`, `NeverUpdatedLease`, five `NeverCrash*` witnesses, `NeverRollback` |
| PP10 order/truth | `Hooks.PP10OrderTruth`: canonical topological order, required predecessor suppression, irreversible publication, ledger-derived reports | `NeverPublished`, `NeverPostFailure`, `NeverSkip`, `NeverReportedEffect`, `NeverRejectedForgery`, `NeverCycleRefused` |

Witness configurations run the **unmutated** transition system and intentionally
assert that a named successful state is unreachable. TLC must falsify that
assertion. Thus an implementation that denies every operation cannot pass on
safety alone. The exact config, expected property and full counterexample are
recorded separately for each witness. [MUTATIONS.md](MUTATIONS.md) maps each
guard mutation to the expected property and corresponding baseline.

These are compositional host contracts, not one enormous product automaton:

- `Authority` assumes a lifecycle-admitted session. Its `ready` bit abstracts
  successful initialization; `Lifecycle` establishes the prerequisite platform,
  verification and negotiation sequence. It observes admission/commit using
  immutable snapshots so later revocation cannot retroactively invalidate an
  earlier authorized action. Commit checks the **current** epoch and grant.
  Already delivered bytes cannot be recalled.
- `Receipts` assumes authority and quota admission from the other models.
  `Request` durably allocates an intent; `Execute` performs the external action;
  `Complete` persists the result. A crash or failed completion preserves unknown
  or partial status. `Reconcile` represents a trusted, evidence-backed result,
  not a retry or an assumption that an arbitrary program is exactly-once.
  `Rotate` archives completed evidence while retaining deduplication identity.
- `Install` establishes trusted artifact identity, which is a host input to
  `Lifecycle`. Install activation selects an artifact; it does not mean runtime
  enablement or consent. CLI/app installers contend on one operation lock.
  Publish immutable bytes before atomically changing the activation record.
  Recovery never executes staging data. Workers lease exact versions, including
  a formerly active version while an update is active.
- `Hooks` consumes host broker receipts and an immutable effective plan.
  `RunHook` can commit an effect and subsequently fail; earlier effects remain
  in the report. Post-hook failure cannot undo an engine publication. Plugin
  assertions, including an invented receipt 3, do not establish an effect.
- `Resources` independently checks each additive quota dimension. Composition
  requires admission to check **all** applicable dimensions before allocating
  or committing. `Isolation` describes allowed facade/build composition; actual
  Cargo closure audits and native process-spawn fixtures remain PL-V obligations.

## Finite domains and refinement obligations

| Model | Exhaustively explored finite abstraction |
|---|---|
| Authority | Two capability classes, all four selection decisions for source and derived provenance, two policy epochs; a valid token and a differing plugin/artifact/root/invocation/session/generation/namespace token for each field; all manifest subsets |
| Receipts | Two simultaneous ledger entries, two argument digests; the second full action key differs in each of artifact/root/invocation/action ID, including equal local IDs across invocations; audit failure before intent or after effect |
| Lifecycle | Five origin/verification classes, confined/unconfined providers, useful/heartbeat/dead/unknown activity, two work units, pending/resolved/dead dependency, short/long elapsed observation |
| Isolation | Core/strict/default/library modes, pack/map/command operations, injected/absent host and enabled/disabled plugin |
| Resources | Frame, queue, content, diagnostics, state, actions, upload, memory, work; representative ceiling 2, host ceiling 3, requests 1–3 and checked wire overhead; modulus 4 only in the overflow mutant |
| Install | Old version 0 and two competing new versions 1/2; all-valid or one invalid candidate/check; seven independent verification checks, revoked versions, lease sets, crash at each pre-activation boundary, explicit rollback |
| Hooks | Two plugins, every directed graph including self/two-node cycles, every required subset, pre/post boundaries, canonical tie-break, success/failure with/without committed effects, all claimed receipt subsets |

Two identity representatives distinguish matching from mismatching fields;
they are not a cryptographic collision model. Two policy epochs expose stale
tokens. Two ledger slots expose independent invocations and collisions, without
claiming proof for unbounded durable history. Two hooks expose order ties,
cycles and required dependencies; arbitrary graph correctness still needs
implementation property tests. Resource units are equivalence classes around a
boundary, not literal production byte limits. Real base64/JSON encoded length,
checked machine integers, process trees and per-OS file identities need their
own conformance tests. No refinement theorem for an unbounded system is claimed.

`TypeOK` accompanies every configuration. Production model actions deliberately
contain no unconstrained worker transition that grants consent, chooses a
trusted publisher, changes its authenticated origin or supplies a host receipt.
Mutations are named constants selecting an explicitly weakened transition;
properties remain unchanged. No mutation is credited merely for flipping the
invariant's truth value.

## PP8 assumptions: progress, not deadlines

`Lifecycle.Spec` explicitly conjoins weak fairness for admission/refusal, spawn,
negotiation, initialization, invocation, dependency resolution, work advancement,
completion, observation, probing/classification, resuming observation after a
wait, stalled cancellation, cancellation acknowledgment/force and reaping.
Every continuously enabled such action must eventually run. User halt and
elapsed-time observations are optional environment actions, not fairness
assumptions needed to rescue a stuck worker.

The liveness claim requires all of the following:

1. Admitted useful work is finite (two abstract units here), each useful broker
   step advances it, and no action replenishes its budget. Infinite progressing
   computations without a finite budget have no termination guarantee.
2. Host/broker/observer scheduling is fair. A live dependency eventually
   resolves successfully or is observed dead. Waiting is not useful progress
   and does not reset a runtime deadline.
3. A non-advancing or unknown worker is probed. For the unattended branch
   modelled here, the caller's fail-closed unresolved-suspicion policy eventually
   permits cancellation after that evidence. An interactive user choosing
   continue forever is outside this termination claim. Unknown OS activity
   stays `unknown`; it is never treated as a measured zero.
4. Cancellation can eventually force process exit and the OS can reap the
   worker/descendant tree (`OsCanReap = TRUE`). The separate
   `LifecycleUnreapable.cfg` checks safety only with this assumption removed;
   its witness requires visible `reapPending`, never fabricated reaping.

Stuttering and arbitrarily delayed observations permit slow work. `elapsed`
can become `long` without cancellation; advancing work is still admitted.
The timer mutant kills advancing work only because elapsed time is long and
violates PP8 safety. The heartbeat mutant mistakes repeated responsiveness
for progress and produces a fair infinite cycle violating PP8 liveness.
No fairness assumption forces the optional user to cancel that cycle.

## Decisions and trust boundary

decision 080 takes precedence over the proposal's original universal pre-exec wording:
third-party enablement and any third-party instruction require proven native
confinement. Only the authenticated containing-distribution `bundled-git`
origin may temporarily use permission-checked brokers and audit while its OS
confinement is completed. An independently installed official override is not
that origin. Both permitted branches must still satisfy grants, privacy, audit,
resource and receipt contracts. Broker invariants do not prove that an
unconfined native bundled process cannot bypass brokers; it is trusted code
in that temporary disposition. Real adversarial OS isolation is a separate gate.

decision 081 fixes the signature port to first-party safe-Rust **Ed25519 verification**
of the domain-separated exact manifest bytes, with authenticated payload size/
SHA-256, schema, pinned trust, compatibility and freshness checks. `signature`
in this model is the trusted result of that verifier, not an implementation of
the group arithmetic. Signature and checksum are independent checks. Signing
exists only in authorized release tooling using an audited library and the
key manager; there is no signing transition or secret in these models.

decision 078 keeps all Git behavior in `plugins/dctx-plugin-git`. The isolation model
has no core Git transition except the deliberate `core-git` mutant. The
engine can produce a core/strict/plugin-free result with zero worker/Git spawns.

## Host transition correspondence

| Host operation / future property test | Model action(s) |
|---|---|
| Effective grant, config narrowing, explicit approval | `Authority.Configure`, `Consent`, `Narrow` |
| Selection change, disable/halt, stale action refusal | `Authority.ChangeSelection`, `Halt`, `Admit`, `Commit` |
| File/output/state reads and administrative recipe access | `Authority.Read`, `Admin` |
| Intent, effect, completion, crash, audit exhaustion | `Receipts.Request`, `Execute`, `Complete`, `CrashOrUncertain`, `AuditFailure`, `RestoreAudit` |
| Replay, reconciliation, retention | `Receipts.Repeat`, `Reconcile`, `Rotate` |
| Enable/pre-exec, hello, ready, invoke | `Lifecycle.EnableOrRefuse`, `Start`, `Negotiate`, `Initialize`, `Invoke` |
| Progress/dependency, probe, cancellation and reap | `Lifecycle.Advance`, `ResolveDependency`, `Observe`, `Probe`, `Classify`, `ObserveWait`, `CancelStalled`, `Halt`, `AcknowledgeOrForce`, `Reap`, `ObserveReapPending` |
| Core operations and explicit plugin composition | `Isolation.Engine`, `Compose`, `SpawnWorker`, `BrokerGit` |
| Request allocation/backpressure/quota release | `Resources.Receive`, `Drain` |
| Install/verify/stage/publish/activate/recover | `Install.Acquire`, `Download`, `Verify`, `Refuse`, `Stage`, `Publish`, `Activate`, `Crash`, `Recover` |
| Trust revoke, launch revalidation/race, leases/removal | `Install.Revoke`, `Revalidate`, `Substitute`, `Launch`, `RefuseLaunch`, `EndWorker`, `Remove` |
| Explicit exact-version rollback | `Install.ApproveRollback`, `Rollback` |
| Plan/order, hooks, engine publication and report | `Hooks.Plan`, `RunHook`, `Publish`, `Report` |

PL-H/PL-I and the other implementation owners must map their real transitions
to these actions and add port/property tests, including a positive control for
each refusal. PL-V combines that evidence with native confinement, strict
closure, signed artifact and protocol conformance checks before promotion.
