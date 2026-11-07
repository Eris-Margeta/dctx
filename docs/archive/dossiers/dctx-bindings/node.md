Historical design record; superseded by [the current documentation](../../../../README.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
Current licensing: all first-party dctx code and documentation, including the desktop app and planned GPUI rebuild, are Apache-2.0, copyright Eris Margeta. Licence and pricing discussions below describe historical proposals.

# Node: napi-rs

DX-E1r · 2026-10-17 · Apache-2.0 (first-party documentation).
Know-before-use dossier for ARCHITECTURE-PROPOSAL §7 DX-E2.
Depends on the public dctx facade and shared closure inventory; no CLI bridge.
Used for the maintainer's approval and a later Node implementation/packaging packet.

## Versions, licences and confidence

**Current stable versions vouched for: none established offline.** No
`bindings/dctx-node` crate or napi-rs pin is present in this repository. `napi`,
`napi-derive`, any `napi-build` and the npm `@napi-rs/cli` tool require separately
verified compatible exact versions; their version numbers need not match.
Do not infer a release from an npm tag or remembered major version. The full
source/feature graph is unresolved, as recorded in [closure.md](closure.md).

Upstream knowledge: napi-rs is MIT-licensed. Exact chosen crate/tool licence
files still need inspection. Our Rust adapter, JavaScript/TypeScript package,
declarations, tests, loader and platform package metadata must be Apache-2.0.
MIT notices remain MIT notices; Apache-2.0 on our package does not relicense the
dependency. If “Apache-2.0 everywhere” excludes MIT-only dependencies rather
than specifying first-party licensing, **napi-rs cannot be selected on this
evidence**. Eris must settle that interpretation before implementation.

Node-API is a stable C ABI supplied by Node; napi-rs wraps it in Rust. An addon
can target a Node-API level rather than each V8 ABI, but Node still supplies a
native runtime. This is third-party FFI, not a pure safe-Rust end-to-end stack.
The facade remains the only engine; no `dctx-cli` crate or executable is linked
as the public binding API.

## Runtime and platform matrix

Choose the lowest Node-API level providing the required task, object and BigInt
APIs, after checking the selected napi-rs release. A Node-API 8 profile is a
candidate, **not a verified minimum or selected feature**. Declare an explicit
Node engine floor consisting of still-supported releases; Node-API compatibility
alone does not make an EOL Node release supported. Do not assert current LTS
numbers without verifying them. Electron needs a separate load matrix even
when its embedded Node offers the selected Node-API level.

| Candidate platform package | Rust target family | Release gate |
|---|---|---|
| Linux x64 glibc | `x86_64-unknown-linux-gnu` | Minimum glibc, ELF imports and Node engine matrix |
| Linux arm64 glibc | `aarch64-unknown-linux-gnu` | Same checks on real arm64 or an approved equivalent environment |
| macOS arm64 | `aarch64-apple-darwin` | Minimum deployment target/SDK, Mach-O imports, supported Node builds |
| macOS x64 | `x86_64-apple-darwin` | Separate native artifact and load test |
| Windows x64 | `x86_64-pc-windows-msvc` | CRT/system DLL policy and PE/Node-API import checks |
| musl, Windows arm64/32-bit, other runtimes | Deferred | Separate binaries and CI evidence; never substitute a glibc binary or invoke a CLI |

An addon is a shared library named `.node`, built as a Rust `cdylib`. It is
loaded by the host Node runtime. ABI stability does not erase OS, CPU, libc or
Node-API feature differences. No WASM, browser or Deno/Bun support is implied.

## Prebuilt distribution without downloads or subprocesses

Ship a small Apache-2.0 JS/type package and exact-version platform packages as
optional dependencies, each containing one prebuilt `.node` file and licence/
notice metadata. Package names are placeholders until namespace ownership is
verified. Constrain `os`/`cpu` and, where supported, `libc`. The loader selects
from a fixed allowlist for platform/architecture/libc and loads only that local
package, validating its binding/API version. Unsupported or omitted optional
dependencies produce a clear binding installation error.

**No install/postinstall script, node-gyp build fallback, shell/libc probe,
curl, CLI download, exec or spawn.** The package manager may fetch declared
registry dependencies during an authorized normal installation; the addon and
loader never fetch anything during import or operation. An offline deployment
uses pre-staged exact package tarballs and a frozen install. Test installation
with scripts disabled and runtime network unavailable. `--omit=optional` is
allowed to yield a typed missing-platform-package failure, not a hidden build.

Use safe in-process OS/runtime information to discriminate glibc versus musl;
do not run `ldd` or a shell. If that information is insufficient, refuse with
explicit platform diagnostics. Review generated napi-rs loader templates:
accepting the generator does not automatically accept its fallback branches.
An explicit developer source-build command is distinct from user installation.

The **single native payload per target** statically contains dctx and approved
Rust dependencies. Runtime resources must be embedded or in the installed
package, never read from the source checkout. Node, libuv/V8 internals and OS
libraries are supplied by the host; import tables must substantiate which ones
the addon directly needs. If a non-system shared library is required, it changes
the proposed payload contract and needs review before bundling it.

The umbrella npm package plus platform tarball is not literally one universal
distribution file. If Eris requires a single installable archive containing
everything, the alternative is one directly installed package per platform;
decide that explicitly. Neither arrangement uses a helper executable or a C
ABI shim of our own. Packaging must not install the Rust `dctx-platform-probe`
test binary merely because a dependency manifest contains that target.

## Synchronous facade behind an asynchronous Node API

Use the [shared contract](README.md#shared-adapter-contract-proposed-for-both-languages):
explicit `SessionBuilder` inputs, bounded ephemeral storage initially, no plugin
host, current request/wire versions and the same Rust golden corpus. Export
Promises for long snapshot/query/pack work. A napi-rs task/async-work abstraction
(historically `Task` with `AsyncTask`) can execute the synchronous facade on
Node's worker pool and resolve/reject on the owning JS environment.

Prefer this worker-task path over adding Tokio solely to call synchronous
functions. An `async fn` that performs blocking work before yielding would
still block its executor; the keyword alone is not an offload strategy. Exact
task APIs, features and cancellation behavior must be checked at the chosen pin.
Bound global admission as well as per-session work so callers cannot fill the
libuv queue indefinitely. Return `Busy` when admission is full. Do not block a
pool worker waiting behind another operation on the same session.

At submission, validate and copy JS input to owned Rust types; reject numbers
that are fractional/out-of-range and preserve u64 quantities as BigInt or
documented decimal strings. Do not hold JS `Env`, object references, borrowed
Buffer slices or callback arguments across a worker boundary. Compute with
Rust owners and a Rust `BoundedSink`; then create JS results on the originating
environment's completion path. Owned copied Buffer output avoids raw external
buffer/finalizer ownership APIs in the first release. Account for both native
and JS result copies in the memory budget.

An `Arc`-owned session and operation guard enforce one active operation. Prove
`Send`/ownership bounds without `unsafe impl`; a JS Worker has its own environment
and must not receive another environment's native handle. Each Snapshot retains
its session owner and generation pin. Compare owner identity before querying,
and reject wrong-session, closed, stale and cross-environment handles.

## Threadsafe functions and progress

Node-API threadsafe functions are the mechanism for safely scheduling JS
callbacks from native threads through the event loop; raw calls to JS from a
worker are invalid. napi-rs offers a safe abstraction, backed by third-party
unsafe. Its exact lifecycle, queue-size, ref/unref and abort semantics are
version-dependent.

**No progress callback in the initial binding.** Tasks need only their normal
completion path. If later approved, use a bounded threadsafe-function queue,
explicit overflow policy, cancellation and environment cleanup. Never block a
worker indefinitely on callback delivery, hold the session lock across a JS
callback, or retain callback references after teardown. A threadsafe function
does not make an arbitrary JS value safe to send between threads. Callback
reentrancy may submit another request, which must obey Busy/Closed rules.

## Cancellation, finalizers and environment teardown

Forward an AbortSignal through the JS wrapper to a **separate Rust cancellation
token** referencing the session flag. Pre-aborted input never starts work.
Install/remove the listener without a missed-abort window, and settle a Promise
once. Unsubscribe on success, refusal, close and abort. The signal must work
without taking the session's work lock. Document that initial abort cancels the
whole session permanently; this matches `SessionBuilder::cancellation` and its
monotonic `CancelFlag`, not a fictitious per-call facade API.

Node-API async-work cancellation can prevent queued work starting; it cannot
forcefully stop a computation already running. Always forward to facade
checkpoints as well. Rejection of a Promise or GC of a Promise is not proof the
native task stopped. Cancellation after the engine commit must preserve the
actual outcome; do not report successful rollback. Worker resources live until
native completion even when the JS consumer has gone away.

`close()` denies new submissions, signals cancellation and allows the JS caller
to await draining. A finalizer merely releases safe Rust ownership/signals
cancellation; it does not join a worker, call JS, launch processes or delete
caches. Dropping a wrapper cannot free the session while a task or Snapshot
retains it. Expose explicit Snapshot close/release and use finalization only as
a fallback: JS GC timing is not a cache lease deadline.

Environment/Worker termination needs the exact napi-rs cleanup contract: no
completion callbacks after an environment is gone, no use-after-free of Env,
and no stale threadsafe-function references. Choose safe supported cleanup
hooks only. Test repeated Worker creation/termination during in-flight work,
GC after submission, last Snapshot release, Session drop before Snapshot,
abort/completion races and process exit. If safe APIs cannot express the
required shutdown, stop rather than introduce raw N-API cleanup code.

## Error and lifetime mapping

Resolve owned answer/report values on success. Reject with a JavaScript
`DctxError extends Error` with exact facade `code`, `retryable`, safe variant
fields and commit information as defined in the shared contract. napi-rs status
codes are transport statuses and must not replace `PolicyDenied`, `Cancelled`,
`SourceChanged`, `StaleContinuation` or other facade codes. If napi-rs's default
error conversion cannot carry fields, use a safe completion conversion and JS
wrapper; do not stringify/parse the error message as a protocol.

Use a separate binding error namespace for malformed arguments, missing native
module, incompatible addon ABI, closed/cross-owner handles and poisoned state.
Preserve bounded/private messages and avoid adding source paths in the loader
or error wrapper. Tests cover every facade variant with an ordinary success
control, not just an invalid-argument rejection.

Audit the exact generated panic boundary: no Rust unwinding through Node's C
ABI. Safe panic containment must release the operation guard and surface a
stable binding failure; poisoned state must be refused explicitly. OOM/abort
or foreign runtime corruption is not a recoverable Promise guarantee. No custom
finalizer/raw pointer reconstruction, leaked `Box`, unsafe lifetime extension
or `napi-sys` call belongs in first-party code.

## Build versus runtime closure

Build tools include the pinned Rust compiler/linker/SDK, napi-derive backend
and proc macros, any napi-build script, and the pinned `@napi-rs/cli` npm tool
with its own lockfile/dependencies. These tools may run compilers in authorized
CI, not at user import/install time. They are not runtime dependencies of the
umbrella package. Generated JavaScript is shipped code and still requires
licence/security/no-fallback review.

Runtime includes the `.node` extension's Rust graph (`napi`, its FFI layer and
the facade), JS loader, platform package, and host Node/system imports. Start
without napi serde/JSON, Tokio, callback/progress or external buffer features.
Whether disabling them removes their crates is a resolved-feature question;
never infer the answer from Cargo.lock's workspace union. closure.md records
what is known and the exact inventory that remains missing.

## Verify online

- [napi-rs source/releases](https://github.com/napi-rs/napi-rs) and
  [documentation](https://napi.rs/): current compatible exact crate/CLI pins,
  licences, MSRV, features, generated unsafe, Task/AsyncTask, AbortSignal,
  threadsafe-function, finalizer and environment-cleanup semantics.
- [Node-API](https://nodejs.org/api/n-api.html) and
  [Node releases](https://nodejs.org/en/about/previous-releases): exact ABI-level
  availability, current supported Node versions and Electron compatibility.
- [npm package metadata](https://docs.npmjs.com/cli/configuring-npm/package-json):
  current libc/platform optional-dependency behavior across supported package
  managers; verify the pinned generator emits no install/download/spawn fallback.
- Resolve all native/build/npm transitive pins and source licences, inspect
  imports, compile with forbid unsafe and run the declared platform/Worker
  matrix. None of these URLs was fetched in this packet.
