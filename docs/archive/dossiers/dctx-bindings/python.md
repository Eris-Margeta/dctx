Historical design record; superseded by [the current documentation](../../../../README.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
Current licensing: all first-party dctx code and documentation, including the desktop app and planned GPUI rebuild, are Apache-2.0, copyright Eris Margeta. Licence and pricing discussions below describe historical proposals.

# Python: PyO3 and maturin

DX-E1r · 2026-10-17 · Apache-2.0 (first-party documentation).
Know-before-use dossier for ARCHITECTURE-PROPOSAL §7 DX-E1.
Depends on the public dctx facade and the shared closure inventory, not the CLI.
Used for the maintainer's binding approval and a later Python implementation packet.

## Versions, licences and confidence

**Current stable versions vouched for: none established offline.** This
repository has neither binding manifests nor PyO3/maturin lock entries. Do not
turn a remembered historical release into a current pin. Exact compatible
versions, their release status, MSRV, checksums and source audits remain a
prerequisite in [closure.md](closure.md), not an implementation choice left to
the worker. No packages or source archives were fetched.

Upstream knowledge: PyO3 and maturin have offered **MIT OR Apache-2.0** licensing.
Select Apache-2.0 where the chosen release permits it and retain required
notices. This is a licence-family statement, not an exact-version inspection.
Our Rust crate, Python wrapper, stubs, tests and metadata must say Apache-2.0.
CPython's PSF licence and OS libraries remain separate host obligations; the
wheel does not relicense or bundle the interpreter.

PyO3 supplies Rust wrappers and generated CPython extension glue. maturin is a
build/packaging tool and PEP 517 backend. Neither replaces the facade or requires
shipping the dctx CLI. An installed wheel need not install maturin, Cargo, rustc
or PyO3 as a Python runtime package.

## Proposed ABI and wheel matrix

Use a PyO3 extension `cdylib`, imported as a private module of the Python
package. Select PyO3's `extension-module`, macros and an explicit `abi3-pyXY`
minimum only after checking the exact release. Disable unnecessary conversion,
async and serialization features. Prefer `abi3-py310` as a **candidate** floor;
Python support/security lifetime and current PyO3 compatibility must be checked
before Eris chooses it. This is not a claim that Python 3.10 is current or supported.

abi3 is CPython's limited/stable C ABI: one compatible binary can cover multiple
CPython minor versions above the chosen floor **on the same platform ABI**.
It is not a universal OS/CPU wheel, a promise of PyPy support, or evidence of
free-threaded compatibility. Test each promised interpreter version. Defer
free-threaded builds, subinterpreters, PyPy and debug-interpreter variants until
their own pinned support and isolation tests exist.

| Candidate wheel targets | Proposed packaging | Still to establish |
|---|---|---|
| Linux x86_64 and aarch64, glibc | Separate manylinux-tagged abi3 wheels | Lowest supported glibc/manylinux policy compatible with Rust, parser and all native imports; build on that baseline, audit final ELF symbols |
| macOS arm64 and x86_64 | Separate deployment-target-tagged abi3 wheels | Minimum macOS/SDK, architecture builds and native loader dependencies; universal2 only if both slices are built/tested |
| Windows x86_64 MSVC | `win_amd64` abi3 wheel | Supported Windows and CRT policy, python3 stable import library and release interpreter matrix |
| Linux musl, Windows arm64/32-bit, other interpreters | Deferred | Separate toolchain/loader/ABI evidence; no fallback to a downloaded CLI |

Do not invent exact wheel tags before the baseline decision. Stable ABI use
must cover every generated Python call, error class and object conversion, not
just set a filename tag. Cross-compilation uses approved target Python metadata
and SDK/import libraries; guessing from the build machine's Python is invalid.

## One self-contained distribution file

The deliverable is **one `.whl` archive per supported target**, containing
Python loader/stubs, one native extension, metadata, licences and notices.
The wheel expands into several installed files; it is not a single executable.
Statically link the Rust facade and its permitted Rust closure into the
extension. Bundle templates/resources or embed them through approved package
resources so no checkout, Cargo registry, git binary or network service is
needed at import or operation time. Do not run git to obtain a version at runtime.

The extension still imports CPython API symbols and permitted OS runtime
libraries. Linux/macOS extensions commonly resolve Python symbols from the
host; Windows needs the matching stable ABI Python DLL/import setup. libc,
libSystem and Windows system/CRT choices must be inspected per artifact.
“Self-contained” means no separately downloaded dctx engine/helper, not a
statically bundled Python/OS or absence of third-party code.

If wheel repair vendors a non-system shared library, its files/licences belong
inside the same wheel and in the audited runtime closure. A second native
library conflicts with the proposed one-extension-file payload and needs an
explicit design amendment; do not silently add a sidecar. Use no sdist fallback
in ordinary install tests: a wheel-only install must fail clearly on an
unsupported target. Source builds, if later offered, explicitly need a Rust
toolchain and build dependencies; they cannot satisfy the wheel-only promise.

## Mapping onto the facade

Use the [shared adapter contract](README.md#shared-adapter-contract-proposed-for-both-languages).
`SessionBuilder` requires explicit ports/policy/clock/grants/storage/place/view;
the Python wrapper must document supplied defaults rather than discover global
configuration. Use a real injected clock for production, not `FixedClock`.
No Python callback implements Source, Cache, Clock or a sink in the first release.
That keeps work on Rust-owned values while the interpreter is detached.

Wrap `Session` in a Rust owner with synchronization; a Python Snapshot contains
an owned clone of `SnapshotHandle` and a reference to that owner. Check owner
identity before facade generation lookup. Returned bytes and strings are owned
Python copies; no memoryview over mutable Rust storage, cached `&str`, raw
pointer capsule, or transmuted lifetime. A snapshot is not a serialization of
the session: do not pickle it as a usable handle.

The session performs one operation at a time because facade methods use
`&mut self`. Reject competing work with `Busy`, not a blocking lock while
holding Python interpreter access. Use safe `Arc`/mutex ownership; prove required
`Send` bounds in CI instead of `unsafe impl Send/Sync`. Move only owned Rust
requests, handles and owners into the work closure. No Python borrow guard or
borrowed Python buffer crosses into detached work.

## GIL release, cancellation and shutdown

PyO3 provides a safe way to release interpreter access around Rust work
(historically `Python::allow_threads`; newer APIs may call it `detach`). Use
the selected version's documented API and its `Ungil`/`Send` restrictions.
Do not guess the signature or silence the bounds. Sequence: validate/copy input
while attached, clone Rust owner, detach for filesystem/parse/query/pack work,
drop work locks, reattach and convert the bounded result/error. Never call Python
or manipulate Python reference counts through raw APIs while detached.

A CPU-heavy facade call must release the GIL even when its Python API is
synchronous. The calling thread remains occupied, but other Python threads can
run. Users may offload the synchronous API to their executor; an asyncio bridge
is deferred so `pyo3-async-runtimes`/Tokio are not presumed dependencies.

Expose a separate cancellation token sharing the session's atomic flag; it
must remain callable while the Session method is active. Calling cancel on a
mutably borrowed pyclass could otherwise be rejected by borrow checking. The
token changes only Rust atomic state without taking the session lock. Initial
cancellation is terminal for that session, matching the monotonic facade port.
Cancel before entry, during snapshot work and during a query; keep a positive
uncancelled control. Cancellation never forcefully kills a Rust/Python thread.

Releasing the GIL alone does **not** poll Python signals during a long Rust
closure. Ctrl-C may be observed only on reattachment. Check pending signals
through the safe PyO3 API on return, document that delay, and test it separately
from the explicit token. Cancelling an asyncio Future or executor waiter does
not itself cancel native work; a wrapper must explicitly forward cancellation.
If prompt signal response is required, add a separately reviewed chunked/polling
design rather than reacquiring Python from arbitrary facade callbacks.

`close()` marks the owner closed, requests cancellation and refuses new work.
In-flight Rust owners retain the session until work stops. Snapshot leases keep
their generation pinned while usable; a closed owner returns a binding Closed
error even if the lease still exists. Context-manager exit provides deterministic
close. GC is a fallback release, not a place to join workers, run Python,
perform filesystem cleanup or delete caches. Test last-handle drop, dropping
the Session before a Snapshot, close during work and interpreter shutdown.

## Error mapping

Provide a documented Python `DctxError(Exception)` carrying stable `code`,
`retryable`, bounded message, safe variant fields and facade commit information.
`Cancelled` may have its own subclass of that error; do not pretend it is
`asyncio.CancelledError` or `KeyboardInterrupt`. Python signal exceptions keep
their native semantics. Request conversion failures use TypeError/ValueError
or documented binding-specific errors before entering the engine.

Preserve every facade error including PolicyDenied/Unavailable, all Unsupported
variants, BudgetTooSmall, ResourceLimit, CacheLimit, Busy, Cancelled, Stale,
StaleContinuation, CorruptIndex, SourceChanged/Unavailable, Io and Hook. Unknown
codes fail visibly; no “empty result” fallback. Privacy errors must not reveal
paths/source text or expand bounded detail with a Rust Debug dump. A signal or
conversion failure after successful pack must not imply engine rollback.

PyO3 has panic handling at its generated boundary, but exact exception behavior
is version-specific. Audit it and exercise a test-only panic without production
`panic!`. Do not let unwinding cross the C ABI. Native aborts, OOM and interpreter
faults are outside a recoverable-exception promise. Avoid holding locks during
Python object creation/finalization to prevent reentrant deadlocks.

## Build and runtime closure

Build inputs: pinned Rust/Cargo, compiler/linker/SDK, target Python ABI metadata,
PyO3 build configuration and proc macros, maturin and its complete own Rust/
Python tool graph, approved wheel audit/repair tools. They are not automatically
wheel runtime dependencies. The source build backend belongs in pinned build
requirements, not runtime `Requires-Dist`.

Runtime: native extension containing facade/PyO3 normal dependencies, Python
package files and any explicitly audited native payload; host CPython and OS
runtime libraries. Maturin itself does not go in the extension. Dev/test tools
and facade dev-dependencies do not belong in runtime. See closure.md for the
current facade graph and unresolved **exact** PyO3 and maturin graphs.

## Verify online

- [PyO3 releases/source](https://github.com/PyO3/pyo3) and
  [maturin releases/source](https://github.com/PyO3/maturin): current stable pins,
  licences/notices, MSRV, advisories, checksums and supported host versions.
- [PyO3 guide](https://pyo3.rs/): exact abi3 feature floor, GIL/detach API,
  pyclass bounds, signal behavior, shutdown/panic handling and generated unsafe.
  Compile the actual module/classes/exceptions with forbid unsafe on every target.
- [maturin guide](https://www.maturin.rs/): current manylinux/musllinux policies,
  Windows import/CRT behavior, macOS deployment tags and wheel repair closure.
- [Python stable ABI](https://docs.python.org/3/c-api/stable.html) and Python
  lifecycle: supported minimum, limited API exceptions, free-threaded distinction.
  Verify artifact imports against declared interpreter/platform combinations.
- Resolve all normal/build/dev/transitive pins, native loader imports and
  source licences as specified in closure.md. These links were not fetched.
