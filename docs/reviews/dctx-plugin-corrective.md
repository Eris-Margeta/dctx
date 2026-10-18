| Unit | Verdict | Findings |
|---|---|---|
| PL-FAUDIT | PASS-WITH-NOTES | F7; no additional correctness finding in the bounded audit codec/storage under its documented trusted-directory assumption. |
| PL-HRECOVERe | FAIL | F2, F3, F7: receipt allocation and new invocation identity are not coordinated with retained evidence. |
| PL-FBINDb | PASS | no finding |
| PL-FSTRICT | PASS-WITH-NOTES | F7; no additional finding in feature wiring, disabled stubs or the disabled-hook controls in `tests/isolation.rs`. |
| PL-CSTORE | FAIL | F1, F6, F7: macOS descriptor traversal fails; Windows qualification remains an ignored placeholder. |
| PL-CSTRICT | PASS-WITH-NOTES | F7; no additional finding in worker/list/run feature-specific refusal controls. |
| PL-RELp | PASS | no finding in the closed target mapping and typed refusal before store access. |
| PL-VISOc | FAIL | F4, F5: lockfile restoration can overwrite concurrent work; Cargo is killed on total elapsed time. |

DX-RV1 independent source review, 2026-10-18. Read the named units and supporting tests in bounded ranges; executed no code, Cargo, tests or reproducer. Reproducers below are deductions from source, not observed runs. References are repository-relative.

Checked against `PLUGINS.md`, PP-MATRIX PP1/3/4/5/6/7/9, the local Rust/code standards, and decision 077/decision 078 with the subsequent explicit plugin exception. The inspected corrective manifests use Apache-2.0 and first-party SHA-256; no new external production dependency, first-party unsafe, production panic, or source-content logging was identified in this scope. This is not a resolved dependency or native binary audit.

## Findings, ordered by severity

### F1 — High: the filesystem store's directory anchoring does not work on macOS

**Location:** `crates/dctx-cli/src/plugin/disk.rs:246` (`entry`), used by `descend` and every store operation.

The non-Linux Unix branch constructs `/dev/fd/<directory-fd>/<child>`. Darwin's `/dev/fd` provides descriptor duplication, not Linux `/proc/self/fd` directory traversal; appending a child component fails with `ENOTDIR`. `root()` immediately uses this mechanism to walk from `/`, so an ordinary macOS store cannot reach its configured root. Even a missing-root `plugin list` returns `Interrupted` rather than the bundled catalog. The tests canonicalize the temporary parent but do not correct this mechanism.

**Minimal reproducer:** On macOS, open `/` as a directory `File`, then call `symlink_metadata(entry(&directory, "Users"))`; the expected existing directory cannot be resolved. At the public seam, `FsStore::new(<canonical temporary directory>/plugins).try_lock()` fails before creating a usable store. `missing_root_read_is_side_effect_free` is already a native positive control that should expose this.

**Suggested fix:** Supply a qualified Darwin descriptor-relative adapter through an approved safe API. Until then, explicitly classify the store as unsupported on Darwin rather than treating all Unix systems as having Linux descriptor-path semantics; qualify the real macOS control before advertising support.

### F2 — Medium: two recovered brokers can create a log that recovery subsequently rejects

**Location:** `crates/dctx/src/plugins/boundary.rs:119`; `crates/dctx-plugin-host/src/domain/ledger.rs:83`; `crates/dctx/src/plugins/audit.rs:25`.

Each invocation restores a separate `Ledger`, whose next receipt number is local. The facade shares the audit through `Arc<Mutex<PortBody>>`, but locks individual audit calls, not the recover/reserve/append transaction. Two brokers can recover the same prefix and both reserve `receipt:1`. `DurableAudit` permits those intents when their invocation IDs differ because its link includes the invocation; `Ledger::restore` rejects the same persisted history because it requires receipt IDs to be globally unique. Subsequent invocations then fail with invalid recovery evidence.

**Exact missing test:** Share one `DurableAudit` between two prepared brokers with distinct invocations and independent fake file ports. Call `recover()` on both while the log is empty, then complete one replacement through each broker. Both return `receipt:1`; recovering their common log into a third ledger fails. This schedule needs no timing or actual threads. Add a control that recovers the second broker after the first completion and obtains distinct receipts.

**Suggested fix:** Coordinate receipt reservation and durable intent insertion through one shared ledger/allocator, or use durable globally unique receipt identities. The audit's acceptance rules and the recovery validator must enforce the same uniqueness scope.

### F3 — Medium: reopening the facade reuses invocation IDs for new operations

**Location:** `crates/dctx/src/plugins/mod.rs:219`; `crates/dctx/src/plugins/boundary.rs:98`; `crates/dctx-plugin-host/src/domain/binding.rs:32`.

`from_body` resets the host counter to zero, including after `platform_with_audit` reopens an existing log. The first new operation again receives `inv-1`, and scripted action IDs repeat. The durable key contains artifact/root/invocation/action, but no session ID. With the same artifact and root, a genuinely new pack can therefore match an older committed key and return its receipt without doing the new effect, or inherit an older unknown refusal. Fresh broker handle/precondition/upload counters can also reproduce the same canonical request digest despite newly staged content. Native launches are currently refused, so this is a recovery-integration defect awaiting native execution, not a claim of a presently successful production launch.

**Exact missing test:** Use the facade's internal fake ports with one persistent audit. Complete the first scripted replacement, destroy the host, reopen its audit in a newly constructed host with the same trusted artifact/root and a new session, and issue a new pack with a different replacement. Assert a distinct invocation and one new effect. Separately reopen an explicitly identified retry and assert that it returns the old receipt without execution. Existing disk tests manually choose invocation IDs and bypass this allocation path.

**Suggested fix:** Allocate new invocation IDs from durable state or an injected restart-unique namespace. Make resuming an old invocation an explicit recovery operation, separate from starting a new pack.

### F4 — Medium: the isolation gate can overwrite a concurrent lockfile update

**Location:** `tools/plugin-conformance/isolation.py:112`, called around all three resolutions in `run_gate`.

The gate runs Cargo without `--locked`, saves the original workspace lockfile, then writes those old bytes back whenever the file differs. It cannot distinguish its own resolution change from another writer's valid update. It also reports dependency verdicts for a potentially modified resolution before restoring the original lockfile, so the accepted graph need not be the graph retained in the checkout. This conflicts with serialized root-file ownership in `PLUGINS.md` §7.3.

**Minimal reproducer:** Enter `_preserve_lockfile` with contents A, have another client replace the lockfile with B while the context is active, and leave the context. The gate overwrites B with A. The current backup test explicitly accepts this unconditional replacement and has no concurrent-writer control.

**Suggested fix:** Run resolutions with `--locked` against the reviewed checkout and refuse a stale lockfile. If re-resolution is necessary, perform it in an isolated authorized workspace and bind the verdict to that workspace's retained inputs, without restoring over another writer.

### F5 — Medium: Cargo liveness uses an unconditional five-minute kill

**Location:** `tools/plugin-conformance/isolation.py:119` and `:130`.

Both offline and online attempts use `subprocess.run(..., timeout=300)` with captured output. A progressing resolution/download is killed solely for total elapsed time, without examining progress, process state, dependency health or a cancellation attempt. decision 077 explicitly makes a wall-clock cutoff a last resort; this gate supplies none of the preceding evidence.

**Exact missing test:** With an injected process/clock, model a Cargo child advancing its download/resolution progress beyond 300 seconds and then returning a valid closure; require successful completion. Pair it with a stalled/dead dependency case that follows the cancellation/escalation policy. Do not add a five-minute wall-clock test to the normal suite.

**Suggested fix:** Observe process/progress evidence through an injected runner and apply the documented liveness policy. Reserve bounded termination for cancellation or unresolved inactivity after the required checks.

### F6 — Low: the Windows qualification test has no filesystem assertions

**Location:** `crates/dctx-cli/src/plugin/store_tests.rs:260` (`windows_replace_and_acl_semantics`).

The test is ignored, and its sole assertion is `cfg!(windows)`. Removing `#[ignore]` on a Windows executor would pass without exercising replacement, directory flushing, crash recovery or ACLs. The implementation currently refuses non-Unix stores, so this is an explicit evidence gap, not an enabled Windows integrity bypass.

**Exact missing test:** An active Windows refusal test must assert that store operations fail without creating/modifying the root. Before enabling the adapter, native tests must publish a complete authorized control and verify old-or-new registry/artifact state at crash cuts plus rejection of permissive ACLs.

**Suggested fix:** Replace the empty qualification placeholder with executable unsupported-platform coverage now. Keep native qualification as an unfulfilled release gate until it has real filesystem assertions and positive controls.

### F7 — Low: reviewed files exceed the requested 300-line threshold

**Locations and complete-file counts:**

| File | Lines | Unit |
|---|---:|---|
| `crates/dctx/src/plugins/audit.rs:301` | 347 | PL-FAUDIT / PL-HRECOVERe |
| `crates/dctx/src/plugins/ports.rs:301` | 341 | PL-HRECOVERe |
| `crates/dctx-plugin-host/src/domain/ledger.rs:301` | 306 | PL-HRECOVERe |
| `crates/dctx-cli/src/plugin/disk.rs:301` | 367 | PL-CSTORE |
| `crates/dctx-cli/src/plugin/ops_tests.rs:301` | 369 | PL-CSTRICT |
| `crates/dctx/tests/plugins.rs:301` | 356 | PL-FSTRICT |

These exceed the threshold explicitly requested by DX-RV1 and the small-file guidance in the Rust/code standards. The 306-line ledger is only marginally over the standards' approximate limit. Header documentation is present in these files.

**Reproducer:** Count lines in each named file; counts above were read directly, without running code or tests.

**Suggested fix:** Split distinct concerns at existing boundaries, such as audit disk storage versus encoding, store target mapping versus persistence, and test groups by behavior. Preserve the existing assertions and module wiring.

## Acceptance limits

PP1/PP4 binding controls include independent authority inputs, admission/commit revocation and unchanged authorized controls; no finding in that corrective logic. Recovery tests do exercise pending/partial/unknown and committed restart paths, but do not cover F2/F3.

The isolation script explicitly qualifies only normal dependency closures. It supplies neither native spawn observation nor a default/strict binary inventory; those broader PP6 obligations remain open as described in PP-MATRIX. Native launch, real Git, process supervision and truthful execution reports remain separate F-WIRE/A-LAUNCH/C-RUN/V-REAL work; their already documented absence is not counted again as a new corrective finding here.
