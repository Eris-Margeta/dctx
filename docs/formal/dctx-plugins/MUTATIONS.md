# Mutation witnesses

PL-M / PLUGINS §7.1 and the PL-C PP1–PP10 contract. Each row names an
explicit faulty transition configuration, the unchanged property it must
falsify, and the unmutated baseline. `suite.tsv` is the executable mapping.

Safety counterexamples require TLC exit 12 and the exact invariant name.
The heartbeat liveness counterexample requires exit 13 and a cycle under
`Lifecycle.Spec` fairness. A parse error or a timeout never qualifies.

| Mutation configuration | Falsified property | Passing baseline |
|---|---|---|
| [Authority-omit-org](mutations/Authority-omit-org.cfg) | `PP1Authority` | [Authority.cfg](Authority.cfg) |
| [Authority-config-consent](mutations/Authority-config-consent.cfg) | `PP1Authority` | [Authority.cfg](Authority.cfg) |
| [Authority-stale-commit](mutations/Authority-stale-commit.cfg) | `PP1Authority` | [Authority.cfg](Authority.cfg) |
| [Authority-after-halt](mutations/Authority-after-halt.cfg) | `PP1Authority` | [Authority.cfg](Authority.cfg) |
| [Authority-hash-excluded](mutations/Authority-hash-excluded.cfg) | `PP2Privacy` | [Authority.cfg](Authority.cfg) |
| [Authority-debug-content](mutations/Authority-debug-content.cfg) | `PP2Privacy` | [Authority.cfg](Authority.cfg) |
| [Authority-omit-content](mutations/Authority-omit-content.cfg) | `PP2Privacy` | [Authority.cfg](Authority.cfg) |
| [Authority-stale-cache](mutations/Authority-stale-cache.cfg) | `PP2Privacy` | [Authority.cfg](Authority.cfg) |
| [Authority-state-namespace](mutations/Authority-state-namespace.cfg) | `PP2Privacy` | [Authority.cfg](Authority.cfg) |
| [Authority-admin-history](mutations/Authority-admin-history.cfg) | `PP2Privacy` | [Authority.cfg](Authority.cfg) |
| [Authority-before-ready](mutations/Authority-before-ready.cfg) | `PP4Binding` | [Authority.cfg](Authority.cfg) |
| [Authority-cross-root](mutations/Authority-cross-root.cfg) | `PP4Binding` | [Authority.cfg](Authority.cfg) |
| [Authority-cross-artifact](mutations/Authority-cross-artifact.cfg) | `PP4Binding` | [Authority.cfg](Authority.cfg) |
| [Authority-cross-invocation](mutations/Authority-cross-invocation.cfg) | `PP4Binding` | [Authority.cfg](Authority.cfg) |
| [Authority-cross-session](mutations/Authority-cross-session.cfg) | `PP4Binding` | [Authority.cfg](Authority.cfg) |
| [Authority-cross-generation](mutations/Authority-cross-generation.cfg) | `PP4Binding` | [Authority.cfg](Authority.cfg) |
| [Authority-cross-plugin](mutations/Authority-cross-plugin.cfg) | `PP4Binding` | [Authority.cfg](Authority.cfg) |
| [Receipts-effect-before-intent](mutations/Receipts-effect-before-intent.cfg) | `PP3Audit` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-audit-full](mutations/Receipts-audit-full.cfg) | `PP3Audit` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-lose-intent](mutations/Receipts-lose-intent.cfg) | `PP3Audit` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-rotate-pending](mutations/Receipts-rotate-pending.cfg) | `PP3Audit` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-key-collision](mutations/Receipts-key-collision.cfg) | `PP5Replay` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-args-conflict](mutations/Receipts-args-conflict.cfg) | `PP5Replay` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-repeat-committed](mutations/Receipts-repeat-committed.cfg) | `PP5Replay` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-retry-unknown](mutations/Receipts-retry-unknown.cfg) | `PP5Replay` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-retry-partial](mutations/Receipts-retry-partial.cfg) | `PP5Replay` | [Receipts.cfg](Receipts.cfg) |
| [Receipts-retry-pending](mutations/Receipts-retry-pending.cfg) | `PP5Replay` | [Receipts.cfg](Receipts.cfg) |
| [Lifecycle-unconfined-third-party](mutations/Lifecycle-unconfined-third-party.cfg) | `PP4Confinement` | [Lifecycle.cfg](Lifecycle.cfg) |
| [Lifecycle-override-exemption](mutations/Lifecycle-override-exemption.cfg) | `PP4Confinement` | [Lifecycle.cfg](Lifecycle.cfg) |
| [Lifecycle-total-runtime-kill](mutations/Lifecycle-total-runtime-kill.cfg) | `PP8Safety` | [Lifecycle.cfg](Lifecycle.cfg) |
| [Lifecycle-halt-keeps-admission](mutations/Lifecycle-halt-keeps-admission.cfg) | `PP8Safety` | [Lifecycle.cfg](Lifecycle.cfg) |
| [Lifecycle-cancel-admission](mutations/Lifecycle-cancel-admission.cfg) | `PP8Safety` | [Lifecycle.cfg](Lifecycle.cfg) |
| [Lifecycle-heartbeat-progress](mutations/Lifecycle-heartbeat-progress.cfg) | `PP8Progress` | [Lifecycle.cfg](Lifecycle.cfg) |
| [Isolation-core-git](mutations/Isolation-core-git.cfg) | `PP6Isolation` | [Isolation.cfg](Isolation.cfg) |
| [Isolation-ambient-discovery](mutations/Isolation-ambient-discovery.cfg) | `PP6Isolation` | [Isolation.cfg](Isolation.cfg) |
| [Isolation-map-elevation](mutations/Isolation-map-elevation.cfg) | `PP6Isolation` | [Isolation.cfg](Isolation.cfg) |
| [Resources-per-chunk](mutations/Resources-per-chunk.cfg) | `PP7Bounds` | [Resources.cfg](Resources.cfg) |
| [Resources-wrap](mutations/Resources-wrap.cfg) | `PP7Bounds` | [Resources.cfg](Resources.cfg) |
| [Resources-omit-wire-overhead](mutations/Resources-omit-wire-overhead.cfg) | `PP7Bounds` | [Resources.cfg](Resources.cfg) |
| [Resources-omit-quota-minimum](mutations/Resources-omit-quota-minimum.cfg) | `PP7Bounds` | [Resources.cfg](Resources.cfg) |
| [Resources-allocate-before-check](mutations/Resources-allocate-before-check.cfg) | `PP7Bounds` | [Resources.cfg](Resources.cfg) |
| [Install-hash](mutations/Install-hash.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-size](mutations/Install-size.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-signature](mutations/Install-signature.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-trust](mutations/Install-trust.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-target](mutations/Install-target.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-schema](mutations/Install-schema.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-freshness](mutations/Install-freshness.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-activate-staging](mutations/Install-activate-staging.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-activate-revoked](mutations/Install-activate-revoked.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-implicit-downgrade](mutations/Install-implicit-downgrade.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-crash-partial](mutations/Install-crash-partial.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-launch-identity](mutations/Install-launch-identity.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Install-unlink-lease](mutations/Install-unlink-lease.cfg) | `PP9Integrity` | [Install.cfg](Install.cfg) |
| [Hooks-ignore-cycle](mutations/Hooks-ignore-cycle.cfg) | `PP10OrderTruth` | [Hooks.cfg](Hooks.cfg) |
| [Hooks-ignore-tie](mutations/Hooks-ignore-tie.cfg) | `PP10OrderTruth` | [Hooks.cfg](Hooks.cfg) |
| [Hooks-run-dependent](mutations/Hooks-run-dependent.cfg) | `PP10OrderTruth` | [Hooks.cfg](Hooks.cfg) |
| [Hooks-post-rollback](mutations/Hooks-post-rollback.cfg) | `PP10OrderTruth` | [Hooks.cfg](Hooks.cfg) |
| [Hooks-forged-receipt](mutations/Hooks-forged-receipt.cfg) | `PP10OrderTruth` | [Hooks.cfg](Hooks.cfg) |

The counterexample trace is retained as `.runs/run.*/mutations_<name>.cfg.log`
on the verification VM. Its companion `results.tsv` records the expected
property, TLC exit and duration; `sha256.txt` identifies the precise inputs.

The unmutated successful-path configurations in `witnesses/` also require an
exact invariant violation, this time of a `Never*` assertion. They establish
reachability, not a safety defect. Their logs follow the same naming convention.
No static expected-outcome file is accepted as a model-checker result.
