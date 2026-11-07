Historical design record; superseded by [the current documentation](../../../guides/PLUGINS.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
Current licensing: all first-party dctx code and documentation, including the desktop app and planned GPUI rebuild, are Apache-2.0, copyright Eris Margeta. Licence and pricing discussions below describe historical proposals.

# Platform dependency and unsafe inventory

PL-R · 2026-10-16 · DX-PLUGIN §7.1 and RUST-STANDARDS dependency boundary.
Repository lock/manifest evidence plus explicitly labelled research candidates.
Input to L1 dependency approval and PL-A; no dependency is added or approved here.

## Evidence classes

**R** = exact version observed in the repository; **K** = exact candidate from
existing knowledge, not downloaded or source-reviewed in this packet. Licence
labels below for external crates are known upstream labels, **not an exact-pin
licence audit**. Cargo.lock provides versions/checksums/dependency names, not
licences, feature-resolved target closures or an unsafe proof. An unresolved
licence/source/feature edge blocks promotion; it is not implicitly approved.

The older [DX-A3 audit](../../design/DEPENDENCY-AUDIT.md) describes an earlier
lock. Its claim that several map packages are missing must not be reused as a
description of this checkout. Similarly, a crate present via desktop or tests
is not approved for the plugin adapter merely by being in Cargo.lock.

## Exact direct candidates

| Component / exact pin | Evidence; known licence | Required role and unsafe/native boundary | Disposition |
|---|---|---|---|
| Rust `std`, toolchain to be pinned by PL-A | Platform runtime; MIT OR Apache-2.0 | Safe basic process/pipe API; internally unsafe and OS native calls. `CommandExt::pre_exec` itself requires an unsafe call; std does not provide a portable sandbox or conditional replace. | Existing baseline, insufficient for confinement |
| `rustix = "=1.1.5"` | R; Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Candidate safe FD-relative IO, openat2, seals and process primitives. Internal unsafe/syscalls; libc backend or linux-raw-sys depending target/features. Namespace/fork/exec APIs must be audited individually; a safe crate name is not a safe launch transaction. | Not approved for PL-A; API/closure review pending |
| `landlock = "=0.4.1"` | K; MIT OR Apache-2.0 | Candidate safe Landlock ruleset builder/restrict API with kernel syscalls underneath; require full enforcement and explicit ABI handling. | Source/pin/features/transitives unverified; not approved |
| `seccompiler = "=0.4.0"` | K; Apache-2.0 OR BSD-3-Clause | Candidate BPF compilation/install; unsafe syscall boundary for applying filters. No libseccomp C requirement intended. Does not by itself provide the proposed one-shot exec supervisor. | Source/pin/features/transitives unverified; not approved |
| `libc = "=0.2.190"` | R; MIT OR Apache-2.0 | Raw OS ABI/FFI, unsafe calls. No native library vendoring implied, but calls system libc/libSystem. Cannot directly implement first-party sandbox setup under forbid unsafe. | Inventory only; not an acceptable direct solution |
| `windows = "=0.62.2"` | R; MIT OR Apache-2.0 | Generated Win32/COM bindings; security/process/handle APIs generally require unsafe calls. Links Windows system DLLs. | No complete safe provider API established |
| `windows-sys = "=0.61.2"` | R; MIT OR Apache-2.0 | Lower-level Win32 FFI, unsafe pointer/handle calls, windows-link import boundary. | No complete safe provider API established |
| macOS Seatbelt safe wrapper | No exact approved crate/version established | `sandbox_init`/related native APIs are FFI; profile support/deprecation and lifecycle need research. | Explicit blocker, not a guessed crate |
| Windows AppContainer/job transaction safe wrapper | No exact approved crate/version established | Safe ownership of token, attribute lists, ACLs, suspended child and cleanup needed over Win32. | Explicit blocker, not a guessed crate |
| Linux namespace/one-shot launch safe composition | No complete exact approved dependency established | Landlock + seccompiler + rustix do not establish safe unshare/fork/exec/user-notification sequencing by themselves. | Explicit blocker |

No bubblewrap, sandbox-exec, shell `unshare`, privileged helper, system service
or C shim is selected as a substitute. Their executable versions/licences and
external artifact inventories would need separate approval and could break the
default one-binary promise. Unsafe cannot be hidden in a new first-party helper.

## Repository lock edges, not a resolved plugin feature graph

These versions are observed, not invented semver ranges:

| Root in Cargo.lock | Immediate recorded edges | Unsafe/native closure implications |
|---|---|---|
| rustix 1.1.5 | bitflags 2.13.2; errno 0.3.14; libc 0.2.190; linux-raw-sys 0.12.1; windows-sys 0.61.2 | Syscall/FFI boundary is unsafe; exact selected backend differs by target/features. |
| errno 0.3.14 | libc 0.2.190; windows-sys 0.61.2 | OS errno/GetLastError plumbing, not pure domain code. |
| windows-sys 0.61.2 | windows-link 0.2.1 | Generated imports/system DLLs; not a bundled C sandbox. |
| windows 0.62.2 | windows-collections 0.3.2; windows-core 0.62.2; windows-future 0.3.2; windows-numerics 0.3.1 | Additional generated interface/lifetime machinery; exact subordinate closure must be frozen before use. |
| bitflags 2.13.2 | serde_core (workspace lock selection) | Lockfile feature union is not evidence that a minimal rustix build needs serialization. |

Known upstream licence families: bitflags/errno/libc/linux-raw-sys/windows-*
are MIT OR Apache-2.0; rustix uses the expression above. Source checks for these
exact future/observed lock versions were not performed. In particular, do not
infer no-unsafe from a pure-looking API or absence of C source. Build scripts,
proc macros, runtime loaders, assembly and system libraries also belong to the
inventory. std and OS kernels remain trusted native code outside the strict
crate's first-party source rule.

For landlock/seccompiler, a complete transitive exact version and licence list
is **unresolved**, not empty. For platform wrappers that do not yet have an exact
candidate, there is no approved implementation path. This is why all third-party
targets remain unsupported rather than receiving an unsubstantiated closure.

## Approval record required before implementation

The owning packet must obtain exact source/checksum and licence files; select
minimal target features; enumerate normal/build dependencies, proc macros,
native libraries and loader resources; identify every unsafe/FFI boundary and
why the API makes it safe for the caller; pin Rust/SDK/build tools and licences;
record vulnerability/audit review date and per-target tests. Record runtime,
build and dev graphs separately. A full workspace lock can contain dev signing
and desktop crates without those being allowed in the shipped plugin runtime.

No approved list entry presently supplies this whole job. The standards require
L1 approval before **adding** unlisted dependencies; this research does not add
them and therefore does not pause for implementation approval. PL-A must not
copy these candidate rows into a manifest as if PL-R approved them.
