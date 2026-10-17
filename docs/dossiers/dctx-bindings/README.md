# dctx Python and Node binding dossiers

DX-E1r · 2026-10-17 · Apache-2.0 (first-party documentation).
Research for ARCHITECTURE-PROPOSAL §7, DX-E1 and DX-E2; no implementation or dependency approval.
Depends on the checked-out facade, architecture decisions and repository dependency evidence.
Eris and packet authors use this dossier to decide scope and cut binding work.

## Recommendation and evidence limits

Implement thin PyO3/maturin and napi-rs adapters to **`crates/dctx`**, with
owned requests/results, explicit source and policy, bounded work, and no CLI,
git, plugin launch or runtime download. Keep every first-party Rust crate at
`#![forbid(unsafe_code)]`. Use safe binding APIs; do not introduce a C shim,
raw pointer API or first-party unsafe helper to make them work.

**Design recommendation, not an approved dependency selection.** No binding
crate exists in this checkout. No PyO3, maturin or napi-rs package is pinned in
Cargo.lock. A current stable version and its complete exact dependency/unsafe
closure cannot be certified from these inputs. Those are explicit blockers
to DX-E1/DX-E2 admission, not empty dependency lists. Neither compilation nor
macro expansion was attempted. A safe API design is plausible; a particular
version's compatibility with `forbid(unsafe_code)` is **unproved**.

Read [Python](python.md), [Node](node.md), and the shared
[dependency, licence and unsafe inventory](closure.md). Each ends with a
**Verify online** section. Facts labelled repository evidence describe this
checkout; upstream architecture/licence knowledge is not a release audit.
No network or cargo command was used for this research.

## Governing contracts and discrepancies

- [Proposal §7](../../design/ARCHITECTURE-PROPOSAL.md) defines DX-E1/E2 and
  [decisions](../../design/ARCHITECTURE-DECISIONS.md) keep capability approval
  separate from facade acceptance. decision 077 leaves first-party unsafe, mmap and
  the C ABI deferred (DX-ARCH-05/13). This dossier does not reopen DX-E3.
- This packet's decision 078 instruction sets **Apache-2.0 for first-party binding
  source, metadata, tests and documentation**, and no git at runtime. It
  supersedes older MIT recommendations and git fast paths in design text.
  Existing third-party licences must remain accurate. In particular, the
  known napi-rs licence is MIT; an Apache-only dependency rule would block it.
- [CODE-GUIDELINES](../../standards/CODE-GUIDELINES.md) and the Rust standards
  require typed refusals, public facade calls, small modules, and explicit
  dependency acceptance. Research adds no dependency and needs no exception.
- [DEPENDENCY-AUDIT](../../design/DEPENDENCY-AUDIT.md) is historical evidence.
  Its missing-facade/stale-lock census is no longer this checkout's graph.
  Current manifests include plugin platform dependencies even with no plugin
  host supplied. See closure.md; do not advertise an unsafe-free closure.
- [bindings/README](../../../bindings/README.md) retains its old schedule,
  core-only diagram and illustrative API. Only licences and a dossier link
  change in DX-E1r. The successor binding target is the facade described here.

## Shared adapter contract proposed for both languages

| Concern | Proposed rule and current evidence |
|---|---|
| Session construction | Supply `FsSource::open` on an explicit root, selection/rule texts, clock, cancellation, ceiling, grants, storage, place and view through `SessionBuilder`. Do not discover home/global config. Start with bounded `StoragePolicy::Ephemeral`; persistent adapters need their own approved closure. |
| Engine calls | `snapshot`, `query` (map/find/neighbors/explain/read), `pack`, template and cache APIs delegate to public facade methods. Copy requests before crossing threads. Reject unsupported features/versions; do not recreate engine logic. |
| Features | Explicitly select `pack,map,templates,fs,segments,cache,syntax-rust`, with facade default features disabled and `wire-json` disabled. `syntax-rust` is required by the accepted parser direction; its current exact pins also need review. Existing `encode_answer` supplies wire output without adding serde to the binding. |
| Serialization | Return bounded, owned bytes plus typed metadata. Preserve `WIRE_VERSION = 2` and `REQUEST_VERSION` from the crate. JS maps arbitrary u64 values to BigInt or documented decimal strings, never rounded Number. Reject malformed UTF-8/paths and out-of-range inputs explicitly. |
| Ownership | Each language Session owns a Rust owner; each Snapshot retains that owner plus a cloned `SnapshotHandle`. Verify owner identity before a query: the facade looks up numeric generations and is not a cross-session handle validator. No raw pointers or borrowed views into generations escape. |
| Concurrency | One active operation per session; contention returns `Busy`, avoiding unbounded queues. Snapshot pins and Rust task references outlive wrapper GC. `close()` denies new operations; release after active work finishes. Never acquire the session lock to request cancellation. |
| Cancellation | First release: cancellation is terminal for the whole session, using a separate shared flag. A new session is needed afterwards. `CancelFlag` is monotonic and injected once at build time. No unreviewed reset races or per-operation cancellation promise. |
| Errors | Preserve `DctxError.code().as_str()`, `is_retryable()`, numeric fields and `output_committed()` where supplied. Binding conversion/closed/owner-mismatch errors have a separate documented namespace. No blanket retry, swallowed errors, debug dumps, denied path or source content in errors. |
| Runtime isolation | No CLI import/download/spawn, git lookup, `.git/` reading, shell execution or plugin host injection. `.gitignore` remains plain-text privacy input. No dependency on a repository being under version control. |

Cancellation is cooperative, not a thread kill. Current facade query entry
checks cancellation; snapshot walking has checkpoints; synchronous parsing and
OS reads may run between them. Current deadline calls do not establish a
hard elapsed-time bound (query compares the same `now`, walk carries a supplied
time). A binding must not claim an interruptible syscall or guaranteed deadline.
If release criteria require finer bounds, cut a facade repair before the bindings.
Likewise, a wrapper must not claim a stronger filesystem race/privacy guarantee
than the source adapter actually supplies.

Do not report “cancelled, nothing committed” just because an abort raced a
completed call. Retain the facade outcome. Pack may publish engine state before
encoding/writing the final page; `output_committed()` is not a universal
transaction journal. Tests must distinguish engine commit from delivery to the
language caller. Safe ordinary Rust panics may be converted at an audited
binding boundary; abort/OOM/native faults cannot be promised recoverable.

## Decisions for the maintainer

1. **Licence interpretation:** confirm Apache-2.0 for all new first-party
   artifacts with retained compatible third-party notices. If every dependency
   must itself offer Apache-2.0, reject MIT-only napi-rs rather than relabel it.
   Existing MIT first-party manifests in the facade closure need separately
   owned corrections before release; this packet cannot change them.
2. **Third-party FFI:** approve or reject each exact PyO3/maturin and napi-rs
   closure after online source/licence review. Their safe APIs still rely on
   unsafe FFI and interpreter/OS code. The existing approved list is insufficient.
3. **Macro-generated unsafe:** decide whether third-party-authored expansion
   in our binding crate is permitted under the first-party rule, and require
   compiler evidence that the exact macros work with `forbid(unsafe_code)`.
   No handwritten unsafe or lint weakening is proposed. If this fails, keep
   the binding blocked; any first-party unsafe exception waits for the maintainer.
4. **Support matrix:** choose the Python minimum and CPython ABI policy,
   Node minimum/Node-API level, OS/CPU/libc baselines and release size budget.
   Proposed initial matrices are in the language dossiers; they are not tested.
5. **Lifecycle/API:** accept explicit sessions, owned buffers, ephemeral
   storage initially, one active operation, terminal session cancellation and
   explicit close. Per-operation cancellation, language port callbacks,
   free-threaded Python, subinterpreters and progress callbacks stay deferred.
6. **Facade closure:** retain/audit currently unconditional platform crates or
   cut a facade-owned feature-isolation packet. Approve the exact `syntax-rust`
   parser closure and resolve capability-report limitations before packaging.
   Disabled plugin execution alone does not remove build dependencies.
7. **Distribution:** accept one wheel archive per target and one `.node` binary
   per npm platform package, with no auxiliary executable or install script.
   CPython/Node and platform system libraries remain required host runtimes.
   Source builds are an explicit developer path, never an automatic fallback.
8. **Admission and release:** authorize research/validation prerequisites and
   exact dependency acceptance before DX-E1/E2. Keep C ABI/mmap and publication
   out of these cuts. Dossier completion is not release approval.

## Proposed packet cuts

These are reviewable future cuts, not dispatched packets or permission to write
outside DX-E1r's own set. Predicates below name **new scripts to implement**;
they do not exist yet and were not executed here.

**Prerequisites.** A network-authorized follow-up owns these four dossier files
and an explicitly named source-evidence directory. It selects exact versions,
records checksums/licences and all target normal/build/dev graphs, audits macro
expansion, and obtains Eris/L1 dependency acceptance. DX-A3 supplies rights and
closure acceptance; DX-D1 supplies the tested facade. Any facade repair or
plugin feature isolation owns `crates/dctx/` separately. Before binding builds,
split a workspace-admission prerequisite from DX-E6: only that authorized owner
may admit binding members and have VM CI refresh Cargo.lock. Do not make E1/E2
depend on final E6 (which already depends on E1/E2), or create an ad hoc nested
workspace to evade ownership. The root manifest is never a binding worker edit.

| Packet | Own files / outputs | Dependencies | `ok` predicates on authorized target CI |
|---|---|---|---|
| DX-E1 Python | `bindings/dctx-py/` only: Cargo.toml, pyproject.toml, Apache-2.0 LICENSE/README, `src/` modules for session/handle/cancel/error, Python package/type stubs, tests and shared-corpus adapters, `tools/verify.py`, `evidence/`, `dist/`, authorized build/temp paths within that own set | DX-D1, DX-A3, approved Python and shared closure dossier, decisions 1–7, workspace admission/lock refresh, any required facade fixes | `python3 bindings/dctx-py/tools/verify.py --suite full` and `test -s bindings/dctx-py/evidence/report.json` |
| DX-E2 Node | `bindings/dctx-node/` only: Cargo.toml/build.rs if approved, package.json and package-manager lock, Apache-2.0 LICENSE/README, `src/` modules, JS loader and types, platform package manifests, tests/corpus adapters, `tools/verify.mjs`, `evidence/`, `dist/`, authorized build/temp paths within that own set | DX-D1, DX-A3, approved Node and shared closure dossier, decisions 1–7, workspace admission/lock refresh, any required facade fixes | `node bindings/dctx-node/tools/verify.mjs --suite full` and `test -s bindings/dctx-node/evidence/report.json` |

The full runners must fail on missing evidence and aggregate: exact locked
closure/licence checks; forbid-unsafe compilation and expansion audit; Rust
lint/format checks on the authorized VM; built-artifact import/load on every
declared target; five-verb and pack parity against Rust golden requests; final
wire byte budgets; positive controls for every refusal; concurrent cancellation,
close/drop/GC and owner mismatch; no-git/no-process/no-runtime-network traces;
and dependency loader inspection. Python additionally proves other Python
threads run during long calls; Node proves event-loop progress, Worker teardown
and unsupported-platform failure without downloads. Trace denied content reads
as well as returned errors. Reports identify artifact digest, toolchain, target,
features and test results. No timing benchmark assertions in the default suite.

Every evidence fixture/script must be named in the cut's `in`/`okRequires`, and
all generated artifacts/temp/cache paths authorized before dispatch. Run each
cut's full suite once at its end; unchanged reruns need `validationRepeats`.
Final DX-E6 then runs the product-wide suite once in its authorized environment,
checks aggregate packaging/rights evidence, and still grants no publication.
For this documentation packet, the full suite is exactly the five declared
file/Apache-2.0 predicates; no local product Cargo suite is authorized.

## Verify online

- Establish exact stable releases, MSRV/toolchain and host support from
  [PyO3](https://pyo3.rs/), [maturin](https://www.maturin.rs/), and
  [napi-rs](https://napi.rs/), recording access date and immutable source refs.
- Complete every language and closure verification list, including licence
  files, advisories, macro expansion and per-target binaries. No link was fetched.
- Check package-name ownership on PyPI/npm before choosing names. No name,
  upload, signing identity, certificate or publication permission is assumed.
