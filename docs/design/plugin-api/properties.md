# PP1–PP10 acceptance and mutation witnesses

PL-C · formal obligations from PLUGINS.md §7.1, traced to §§1–6.
PL-M owns models/checker configurations; PL-H/PL-I own corresponding property
tests; PL-V collects their evidence with real platform and closure checks.
These are specifications, not a claim that a model checker has run.

Let `G(t)=M∩U∩O∩H∩S(t)∩A`, `K=(artifact,project,invocation,action_id)` and
`D(args)=SHA256(canonical(method,params-without-action_id))`. `Intent(K)` is a
durable host record. `Commit` includes externally observable effect completion,
not a plugin success string. `Delivered` includes source, cached/derived bytes
and content-bearing state. Counts below are spy observations from fake ports;
platform denial evidence is a separate obligation.

| Property | Invariant / temporal predicate | Accepted control and mutation witness | Owners / corpus |
|---|---|---|---|
| **PP1 authority only narrows** | Every admitted and committed action is in G(now), with current epoch and remaining minima of quotas. Revocation closes admission before another broker action. Config/update never widens consent. | Control: scoped authorized replacement succeeds. Mutation: omit organization intersection or commit-time epoch check; denied replacement/old handle is accepted and the invariant fails. | PL-M/H/D/I; grant-intersection, revoked-epoch, file-race, git-staging-grant pairs |
| **PP2 no excluded source access** | For Exclude/OmitContent/DebugVisible sources, read/hash/delivery counts are zero. Every output/state provenance constituent is currently authorized; administrative access never delivers excluded source/history. | Control: Include read returns abc. Mutations: hash before selection, serve revoked cached output, treat DebugVisible as Include, cross a state namespace, or relabel history as admin metadata. Each yields a counterexample. | PL-M/H/A/G; excluded-source, omitted-source, debug-source, cached-provenance, state-namespace; real Git privacy fixtures |
| **PP3 effect audit** | Commit/delivery implies prior durable intent; completion or uncertainty links to it. Audit unavailable implies no new effect/delivery. Unresolved intent cannot rotate away. | Control: intent then one effect then completion. Mutations: effect before intent, admit with full log, discard pending intent after completion-log failure. Crash cut between effect/completion remains unknown. | PL-M/H/I; durable-intent pair, receipt shape; fault injection |
| **PP4 protocol binding** | Effect implies verified/bound executable, admitted confinement disposition, negotiated compatible version, initialized active invocation and matching root/session/epoch/handle. Third-party enabled implies proven pre-exec confinement. | Control: valid handshake → ready → scoped action. Mutations: serve during initialize, reuse session/handle across root/artifact, trust handshake identity, or let installed override claim bundled exemption. No effect before ready. | PL-M/P/H/A; pre-ready-effect, third-party-confinement, bundled-exception; wire binding negatives |
| **PP5 no blind replay** | Within a reconciled ledger, equal K,D executes at most once and returns the existing receipt; equal K with unequal D refuses. Partial/unknown never auto-retries. | Control: duplicate committed action returns receipt with zero new effects. Mutations: drop key/argument comparison, treat pending as absent or auto-retry after unknown program completion. Recovery cannot claim exactly-once across an unconstrained crash. | PL-M/H; replay-key/unknown-replay pairs, wire-error-unsafe-retry |
| **PP6 strict isolation** | Strict core/distribution closure has no host/worker/Git execution. Plugin-free library/pack/map operations have zero plugin/Git spawns and no ambient discovery. Read-only map preserves its caller ceiling. | Control: explicitly injected default host may run eligible worker; strict/plugin-free operation still returns engine result with no worker. Mutations: restore core git_safety, lazy global discovery, or elevate map through before-index. | PL-M/F/W/CLI/V; strict-isolation, library-isolation, read-only-map; closure/source audit |
| **PP7 bounded resources** | Length/allocation, queues, outstanding work, diagnostics, decoded/delivered bytes, uploads/state/actions/memory/work remain within admitted bounds. Checked arithmetic never wraps. | Control: exact byte-budget read succeeds. Mutations: allocate before checking length, account only per chunk, forget base64/envelope overhead, allow queue/state growth or arithmetic overflow. | PL-M/P/H/A; byte-budget pair, frame/JSON/bounds negatives; flood and quota property tests |
| **PP8 lifecycle progress** | Under finite admitted work, fair observations/broker scheduling, eventual dependency resolution and an OS able to reap, every worker eventually completes, refuses, or cancels/reaps. Slow advancing work is not killed solely for total elapsed time; halt revokes immediately. | Control: advancing count after long injected interval survives. Mutations: fixed total-runtime kill, count heartbeats/output spam as useful work forever, ignore dead wait/exit or admit work after cancel. Reap-refusing OS produces visible reap_pending, not a fabricated success. | PL-M/H/A; progress-not-timer/cancel-revocation, progress regressions; fake observation/descendant traces |
| **PP9 install integrity** | Active/launchable implies complete verified compatible trusted artifact and bound launch identity. Interrupted/concurrent activation exposes complete old or complete new only. Leased versions persist. Revoked/downgraded state never activates implicitly. | Control: verified candidate atomic publish activates; pre-publish crash keeps previous. Mutations: activate staging path, omit hash/signature/trust/target check, unlink leased version, or permit check/exec replacement. | PL-M/I/A/REL; install-integrity/activation-crash, artifact failures; signed bundles and Platform |
| **PP10 order and truth** | Effective hook graph is acyclic and deterministic, obeys predecessor failure policy and immutable plan. Published output stays committed after post-hook failure. Reported effects equal host ledger evidence. | Control: complete current rules cause no effects, authorized action gives real receipt. Mutations: ignore cycle/tie-break, run required dependent after failure, claim rollback after publication, trust forged receipt or current marker with missing rule. | PL-M/D/H/F/G; dts cycle controls, output-commit/forged-receipt, marker-idempotency pair |

## decision 080 interpretation of PP4

`admitted_confinement_disposition` is either tested pre-exec confinement, or
the explicit temporary official-bundled-Git disposition authenticated by the
containing distribution. Only the latter may use pending OS confinement. It
retains permission-checked brokers, audit, consent and selection requirements.
Third-party enablement always requires the first branch. Publisher trust,
canonical ID, an installed official build and an override are not the exception.
Model this as a host-established input with no worker transition that sets it.

Keep PP2 broker/selection invariants for both branches. State the assumption
that an unconfined bundled process is not an adversarially isolated OS process;
do not use mock broker tests to claim it cannot perform arbitrary native IO.
The full native adversary property applies only to tested providers. Record
pending OS work and report the exception's status honestly. This is decision 080's
explicit boundary, not permission to loosen third-party/privacy declarations.

## Nonvacuous validation and release evidence

Each safety model configuration must reach the enabled action/commit transition
before proving its refusal cases. Include at least two plugins, roots, policy
epochs, invocations, argument digests and activation candidates where relevant;
a single fixed identity cannot find cross-binding/replay/order bugs. Model
audit failure both before intent and after mutation, cancellation races with
commit, and crash cuts at each installation publication transition. Map every
host transition to an actual model action and every predicate to a property test.

For each mutation above, record the mutation config, falsified property and
counterexample trace, plus the corresponding unmutated check result. Mutation
tests that merely fail syntax or never reach the changed transition do not
count. Schema corpus cases likewise must pass their positive controls before a
negative case can be credited. No blanket “all PP passed” without per-property
witnesses. A directory check or expected-outcome JSON is never such evidence.

PP8 fairness/finite-work/dependency/reaping assumptions must be explicit model
assumptions, not silently guaranteed by the implementation. Model unknown CPU/IO
as unknown. Separate observation cadence from elapsed-runtime policy. Use
injected clocks, fake process activity and deterministic budgets in normal tests;
platform cleanup and isolated benchmarks have their own authorized runners.

PL-V links model/property/schema/signed-trust/native-platform/closure evidence,
then runs its once-only authorized remote workspace suite and separately scoped
desktop checks. decision 081 verification and decision 080 platform disposition remain release
conditions; none of their implementation proof is claimed by PL-C.
