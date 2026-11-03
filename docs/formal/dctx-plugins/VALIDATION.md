# PL-M validation receipt

Observed remote TLC results for the models and configurations in this directory.
PLUGINS §7.1 / PL-C PP1–PP10; used by PL-H/PL-I and the PL-V release gate.
The authoritative detailed inputs and counterexamples are in the VM evidence folder.

Completed 2026-10-16 at 10:54 UTC using the packet's exact remote command:

```sh
sh docs/formal/dctx-plugins/check.sh
```

Exit status: **0**. TLC 2.19 (08 August 2024, revision 5a47802), Java 21 on
the authorized Linux VM. The nonempty-check predicate also returned zero.

```text
PASS PP1-PP10: 8 baselines, 59 mutants, 37 reachability witnesses; 104 total
```

| Baseline | Distinct states | Complete graph depth |
|---|---:|---:|
| Authority | 15,155 | 10 |
| Receipts | 32,940 | 14 |
| Lifecycle, including PP8 temporal check | 2,440 | 15 |
| Isolation | 140 | 5 |
| Resources | 118 | 5 |
| Install | 1,426,900 | 32 |
| Hooks | 3,496 | 6 |
| LifecycleUnreapable, safety only | 2,440 | 15 |

Every baseline exhausted its queue. All 58 safety mutants violated their
named property; the heartbeat-progress mutant produced the expected temporal
counterexample. All 37 unmutated `Never*` assertions produced reachable-state
witnesses. The per-case mapping is [suite.tsv](suite.tsv); mutation descriptions
and baseline links are in [MUTATIONS.md](MUTATIONS.md).

VM evidence directory (remote, not a local checkout path):

```text
<evidence-directory>/docs/formal/dctx-plugins/.runs/<run-id>
```

It contains `results.tsv`, `sha256.txt`, Java/settings records and the full
TLC log for every configuration, including every counterexample. No `--pull`
was used. The remote wrapper reported that its optional external state log was
not writable in the worker sandbox; this did not affect the command's zero
exit or these owned evidence files.

Two earlier attempts stopped in Install before completing the suite: a mixed
string/numeric sentinel and a partial-map lookup in an action guard were model
encoding errors. Both were corrected before this complete run. No unchanged
suite rerun was performed, and no cargo command was run locally or remotely.

This receipt proves the bounded checks described in [README.md](README.md),
not implementation refinement, cryptographic correctness or native confinement.
