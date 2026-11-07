Historical design record; superseded by [the current documentation](../../../../README.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
Current licensing: all first-party dctx code and documentation, including the desktop app and planned GPUI rebuild, are Apache-2.0, copyright Eris Margeta. Licence and pricing discussions below describe historical proposals.

# Binding dependency, licence and unsafe closure

DX-E1r · 2026-10-17 · Apache-2.0 (first-party documentation).
Inventory for ARCHITECTURE-PROPOSAL §7 DX-E1/E2 and decision 077 unsafe decisions 5/13.
Depends on repository manifests/lock, earlier audits and labelled upstream knowledge.
Used to block unreviewed dependencies and inform Eris; this is not a resolved SBOM.

## What can be exact offline

**R** means read from this repository; **K** means upstream architecture/licence
knowledge, not exact-release source inspection; **U** means unresolved. A lock
entry proves only that the repository records that version/edge. It does not
prove registry availability, source licence, advisories, compiled features,
target selection, native imports or absence of unsafe. No registry source was
read or fetched in DX-E1r, and no cargo command was run.

For both bindings the exact new dependency closure is **U, not zero**. PyO3,
maturin, napi, napi-derive, napi-build and @napi-rs/cli are absent as binding
pins. There is no defensible exact transitive list for an unselected release.
The tables below name known component families and exact **existing facade**
lock edges without pretending they resolve the future bindings. This limitation
blocks dependency approval, but not the present documentation deliverable.

## Shared first-party graph (R)

Proposed facade features for both bindings: defaults disabled;
`pack,map,templates,fs,segments,cache,syntax-rust`; no `wire-json`. Current
[facade manifest](../../../../crates/dctx/Cargo.toml) is authoritative for edges.
Every first-party crate below declares `forbid(unsafe_code)` in its lib.rs.
That says nothing about third-party code or unbuilt macro configurations.

| Exact crate/version | Direct normal path edges in its manifest | Licence field observed (R) |
|---|---|---|
| dctx 0.1.0 | core, hash, map-model (defaults off), pack, ports, template, plugin-host, plugin-platform, plugin-protocol; optional lex-rust enabled here (all prefixed `dctx-`) | MIT; conflicts with packet's Apache-2.0 release requirement |
| dctx-core 0.25.0 | dctx-hash | Apache-2.0 |
| dctx-hash 0.1.0 | none | MIT; correction needed |
| dctx-map-model 0.1.0 | dctx-hash; optional serde/serde_json excluded by selected features | Apache-2.0 |
| dctx-pack 0.1.0 | dctx-core, dctx-hash, dctx-ports | Apache-2.0 |
| dctx-ports 0.1.0 | dctx-core | Apache-2.0 |
| dctx-template 0.1.0 | dctx-core, dctx-hash | Apache-2.0 |
| dctx-plugin-host 0.1.0 | dctx-hash, dctx-plugin-protocol | MIT; correction needed |
| dctx-plugin-protocol 0.1.0 | none | MIT; correction needed |
| dctx-plugin-platform 0.1.0 | dctx-hash, dctx-plugin-protocol; external crates below | MIT; correction needed |
| dctx-lex-rust 0.1.0 | no normal path edges; external parser crates below | Apache-2.0 |

Bindings would directly depend on `dctx` plus their binding framework. If an
adapter needs a port type that the facade does not re-export, any additional
direct path dependency must be named in its cut, not silently added. Propose
using existing facade exports, including `memory::CancelFlag`.

The plugin platform dependency is **unconditional**, including with facade
default features disabled. Its Linux dependencies remain in the build graph
even when `SessionBuilder::plugins` is never called. Dead-code elimination is
not a substitute for dependency approval. A separate facade-owned feature split
could remove them; this packet neither edits that graph nor assumes the split.
Native bindings must report their own FFI boundary alongside facade capabilities:
the facade's `native_code=false`/`unsafe_boundary` fields do not describe the
entire wheel/addon or even enumerate the unconditional platform dependency.

The original [audit](../../design/DEPENDENCY-AUDIT.md) predates these manifests.
Its syn 2.0.114 source inspection cannot prove anything about the current syn
2.0.119/3.0.9 lock versions. The platform dossier's older libc 0.2.190 and
landlock/seccompiler candidates are likewise superseded as **observations** by
the current lock, not newly approved by this document.

## Existing external facade edges (R versions; K licences; U source audit)

Direct external declarations:

- `dctx-lex-rust`: proc-macro2 `=1.0.107` (defaults off, span-locations),
  quote `=1.0.47` (defaults off), syn `=3.0.9` (defaults off, full/parsing/
  printing/visit), thiserror `=2.0.18`.
- `dctx-plugin-platform`: thiserror `=2.0.18`; Linux-only rustix `=1.1.5`
  (fs/process/thread/stdio plus defaults), landlock `=0.4.7`, seccompiler
  `=0.5.0` (defaults off). No binding requests a sandbox launch at runtime.

Below is the recursive **recorded lock-edge union** from those roots, excluding
first-party dev edges. It includes target/feature edges that may be inactive.
`A/M` abbreviates the known licence family `MIT OR Apache-2.0`, not a verified
licence of the observed exact version. Rows marked U require actual files.

| Crate exact version (R) | Immediate recorded lock dependencies (R) | Licence knowledge / unsafe locus to audit |
|---|---|---|
| proc-macro2 1.0.107 | unicode-ident 1.0.27 | A/M (K); compiler-token/fallback representation and spans. Exact unsafe sites U; needed as runtime parser support and as macro tooling in separate build units |
| quote 1.0.47 | proc-macro2 1.0.107 | A/M (K); token construction; exact unsafe presence U, do not infer safe from purpose |
| syn 3.0.9 | proc-macro2 1.0.107, quote 1.0.47, unicode-ident 1.0.27 | Exact release licence/unsafe U; historical syn family A/M (K). Runtime parser with cursor/token-buffer internals to audit; version existence/support needs online verification |
| syn 2.0.119 | proc-macro2 1.0.107, quote 1.0.47, unicode-ident 1.0.27 | A/M family (K); host proc-macro parser; unsafe token-buffer/cursor implementation is known historically, exact locations U |
| unicode-ident 1.0.27 | none | Known family `(MIT OR Apache-2.0) AND Unicode-3.0` (K); exact SPDX/text/data notices and unsafe U |
| thiserror 2.0.18 | thiserror-impl 2.0.18 | A/M (K); runtime helper and generated Error impls; exact unsafe and expansion U |
| thiserror-impl 2.0.18 | proc-macro2 1.0.107, quote 1.0.47, syn 2.0.119 | A/M (K); host procedural macro; emitted code must be audited separately from generator |
| rustix 1.1.5 | bitflags 2.13.2, errno 0.3.14, libc 0.2.191, linux-raw-sys 0.12.1, windows-sys 0.61.2 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` (K); unsafe syscall/FFI, FD ownership and target backend, assembly possible; exact sites U |
| landlock 0.4.7 | enumflags2 0.7.12, libc 0.2.191, thiserror 2.0.18 | A/M (K); unsafe Linux Landlock syscalls/FD conversion; exact sites U |
| seccompiler 0.5.0 | libc 0.2.191 | `Apache-2.0 OR BSD-3-Clause` family (K); unsafe filter installation/syscalls; exact release licence/sites U |
| enumflags2 0.7.12 | enumflags2_derive 0.7.12 | A/M (K); unchecked flag construction/invariants and generated implementations require audit; exact unsafe sites U |
| enumflags2_derive 0.7.12 | proc-macro2 1.0.107, quote 1.0.47, syn 2.0.119 | A/M (K); host macro and its emitted code; exact unsafe U |
| libc 0.2.191 | none | A/M (K); raw C ABI declarations and unsafe calls, system libraries; exact target source sites U |
| linux-raw-sys 0.12.1 | none | A/M family (K); kernel ABI constants/types and generated declarations; exact unsafe, inline code and notices U |
| errno 0.3.14 | libc 0.2.191, windows-sys 0.61.2 | A/M (K); unsafe errno/GetLastError access; exact sites U |
| bitflags 2.13.2 | serde_core 1.0.228 | A/M (K); flag representation/macros; exact unsafe U. Serde edge is workspace feature union, not proof it is active here |
| windows-sys 0.61.2 | windows-link 0.2.1 | A/M (K); generated raw Win32 ABI declarations/imports; target-specific, not a Linux DLL payload |
| windows-link 0.2.1 | none | A/M (K); linker/import macro expansion and system DLL boundary; exact unsafe/sites U |
| serde_core 1.0.228 | serde_derive 1.0.228 | A/M (K); serialization internals contain unsafe historically; precise selected sources U; lock-feature tail only |
| serde_derive 1.0.228 | proc-macro2 1.0.107, quote 1.0.47, syn 2.0.119 | A/M (K); host macro and emitted code, exact unsafe U; lock-feature tail only |

These are exact repository numbers, **not a claim they are latest stable or
available upstream**. No binding resolver ran. On macOS/Windows the Linux roots
are cfg-disabled; their target edges above do not become direct Windows binding
dependencies just because they occur in Cargo.lock. Source metadata and CI must
classify each selected normal, build/proc-macro, target and dev build unit.
Changing features to `wire-json` adds serde/serde_json and their closure; that
profile is excluded here and would require a new inventory. Dev-only proptest,
tempfile, criterion, sha2 and parser oracle crates are not shipped requirements.

## Python framework and tool graph (K roles; U exact pins/closure)

| Candidate component | Edge/class | Known licence family and unsafe boundary |
|---|---|---|
| pyo3 | Binding direct normal dependency | MIT OR Apache-2.0 (K); safe API over interpreter pointers, refcounts, borrowed/owned objects, exception handling, GIL/thread state; internal unsafe |
| pyo3-ffi | Framework transitive normal dependency | MIT OR Apache-2.0 (K); raw CPython C ABI declarations and unsafe calls; links/imports host CPython |
| pyo3-macros | Framework macro feature / host proc macro | MIT OR Apache-2.0 (K); module/class/function glue generation; emitted unsafe may execute inside our artifact |
| pyo3-macros-backend | Transitive host macro implementation | MIT OR Apache-2.0 (K); code generator, emitted ABI trampolines/conversions/ownership code require separate audit |
| pyo3-build-config | Build/host support (exact edges depend on pin) | MIT OR Apache-2.0 (K); detects Python ABI/link configuration; build IO/tool execution and exact unsafe U |
| maturin | PEP 517 backend/build executable; not a library runtime edge | MIT OR Apache-2.0 (K); packaging, linking/audit/repair; its own entire Rust/Python/native/tool graph is U |
| Macro/support/target transitives | U, release/features not selected | proc-macro2/quote/syn and possibly libc, once_cell, memoffset, indoc/unindent, target import-library helpers are known historical families, **not an exhaustive graph or selected dependencies** |

No exact transitive version, complete licence set or source-line unsafe list
can honestly be supplied for these absent artifacts. Old PyO3 graphs differ
substantially across releases. Maturin's dependencies are not PyO3's runtime
closure and cannot be omitted from the build supply-chain review. Do not
automatically add serde, numpy, Tokio or an asyncio bridge.

## Node framework and tool graph (K roles; U exact pins/closure)

| Candidate component | Edge/class | Known licence family and unsafe boundary |
|---|---|---|
| napi | Binding direct normal dependency | MIT (K); unsafe Node-API calls behind safe value/object/task wrappers, environment ownership, buffer and reference/finalizer handling |
| napi-sys | Framework transitive normal dependency | MIT (K); raw Node-API C ABI and possibly platform symbol loading; exact feature behavior U |
| napi-derive | Direct host proc-macro dependency | MIT (K); emits exports, callback trampolines, conversions, class registration and finalization code; emitted unsafe audit required |
| napi-derive-backend | Transitive host code generation | MIT (K); exact generator and emitted-code unsafe sites U |
| napi-build | Candidate direct build dependency if required by selected toolchain | MIT (K); link/export setup; exact presence and build closure U, not assumed mandatory on every target |
| @napi-rs/cli | npm dev/build tool, not crate runtime dependency | MIT (K); own exact npm lock graph, native dependencies and loader templates U |
| Macro/runtime/platform transitives | U, release/features not selected | Known families include proc-macro2/quote/syn, bitflags, ctor, libloading, once_cell and optional Tokio/serde; **not an exhaustive list or assertion of inclusion** |

The prebuilt platform packages contain the binding artifact; they are our
Apache-2.0 packages with third-party notices, not independent napi-sys builds.
Node itself, its Node-API/libuv machinery and OS system libraries are runtime
trust dependencies outside Cargo. No first-party raw `napi-sys` calls are
proposed. Excluding optional async-runtime/progress features does not eliminate
the native async-work/FFI machinery used by worker tasks.

## Whose unsafe, and can first-party code forbid it?

| Layer | Authorship/execution | decision 077 disposition |
|---|---|---|
| Existing facade/domain/adapter source | First-party; all listed lib.rs headers forbid unsafe | Keep those lints; no blanket claim about std/dependencies. Safe APIs can still have logic/privacy bugs |
| Proposed handwritten binding adapter | First-party requests, ownership, errors and safe task/GIL calls | Zero unsafe blocks/functions/impls/unsafe attributes or unchecked conversions. Keep `forbid(unsafe_code)` and prove all Send/Sync requirements; no lint allow |
| PyO3/napi raw FFI and safe-wrapper implementation | Third-party authored crate code, linked into our shared library | Explicit exact-version third-party unsafe approval required; safe calls do not make the closure unsafe-free |
| Procedural macro output | Third-party-authored generator emits code compiled in **our binding crate**, including ABI entrypoints/unsafe operations | Neither call it handwritten first-party unsafe nor hide it as a separate dependency. Eris must decide treatment; compiler lint behavior depends on version/spans/attributes and is not a source audit |
| First-party build.rs or generated wrapper/shim we author | First-party even if generated or put in another crate | Cannot hide raw FFI here. Any unsafe implementation remains deferred until the maintainer decides |
| rustc/std, CPython or Node, OS/loader | Toolchain/third-party native runtime, contains unsafe/C/C++/assembly | Trusted external runtime, separately documented; not covered by our crate lint |

**Conditional safe design:** both frameworks offer safe user-facing bindings
over native FFI. Owned values, safe classes/tasks, copied buffers and no custom
conversion traits requiring unsafe can plausibly keep handwritten code entirely
safe. Keep `forbid(unsafe_code)` in the actual extension crate, not just an
inner helper. However, **no exact compatible release/expansion is proven here**;
we cannot certify that either complete binding currently satisfies that lint.

The validation prerequisite must compile a representative module with the real
Session/Snapshot/cancellation/error/async/finalizer exports under forbid unsafe
on every selected target, and inspect macro expansions including exported
symbols/unsafe attributes. Some external macro spans may escape a lint;
passing compilation alone does not prove no generated unsafe. Record both
authorship and compiled location. Never replace forbid with allow/deny, use a
handwritten `extern`/raw pointer trampoline, or add a first-party C ABI to work
around a failure. If the chosen macro/API requires first-party unsafe or
incompatible lint allowances, **that binding cannot proceed under the current
rule**. Defer it to Eris; DX-E3 and mmap remain deferred independently.

## Required exact-closure evidence before admission

Freeze each target/toolchain/interpreter, dependency version and feature set.
For every direct/transitive normal, build/proc-macro, dev, npm/Python tool and
native package record: parent edge, exact version/source/checksum, target/cfg,
enabled features, SPDX expression and licence/NOTICE file, whether it executes
at build/runtime, and unsafe/FFI/assembly locations with immutable file/line refs.
Distinguish unsafe in sources, macro-generated code and linked binaries. Include
the complete maturin and @napi-rs/cli tool graphs, not just a runtime Cargo tree.

Use authorized CI for resolution/feature trees, source review, macro expansion,
forbid-unsafe compilation and actual wheel/addon import inspection. Record
system loader imports separately from statically linked Rust crates; build
scripts and repair/vendoring tools can change the binary closure. Record
unresolved cells as blockers rather than asserting no unsafe. Preserve
third-party licences; Apache-2.0 first-party metadata does not remove their
conditions. Existing facade MIT metadata corrections need their own owners.

## Verify online

- Fetch immutable sources/checksums and exact licence/NOTICE files for every
  observed version above and the selected PyO3/maturin/napi-rs/tool versions;
  validate release existence, support/MSRV and advisories. Current lock text
  alone is not an online attestation.
- Use upstream [PyO3](https://github.com/PyO3/pyo3),
  [maturin](https://github.com/PyO3/maturin),
  [napi-rs](https://github.com/napi-rs/napi-rs),
  [crates.io](https://crates.io/) and npm release metadata. No URL was fetched.
- Resolve exact per-target closures, including optional/build dependencies,
  licence data such as Unicode notices, dynamic loaders and all macro expansion
  unsafe sites. Replace U cells with evidence before Eris approves either binding.
- Verify that a source-reviewed, forbid-compatible safe API exists for every
  required lifecycle operation. If none does, record the binding as blocked;
  do not invent a first-party unsafe exception or a C ABI substitute.
