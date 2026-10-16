# PL-Ab source review and unsafe inventory

Reviewed 2026-10-16 with the opensrc skill, exact approved source versions fetched
into a crate-owned cache. Automatic policy review refused cache deletion because
PL-Ab has no delete effect; `.research/` is ignored and excluded from packaging,
and awaits orchestrator cleanup. This supplements
the older PL-R dossier within PL-Ab's ownership; it does not alter its release
qualification matrix or grant any new dependency.

| Dependency | Features/use | Licence and unsafe boundary |
|---|---|---|
| rustix =1.1.5 | std, fs, process, thread, stdio; openat2, fstat, memfd seals, resource limits, parent death | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT. Safe FD ownership at our calls; upstream linux_raw assembly/syscalls or libc backend contains unsafe. Runtime/fork/exec and unshare unsafe APIs are not used. |
| landlock =0.4.7 | Default API, strict ABI4 filesystem and TCP rights, no path exceptions | MIT OR Apache-2.0. Upstream libc/syscall FFI and rule serialization contain unsafe; first-party calls are safe. No best-effort degradation. |
| seccompiler =0.5.0 | No JSON feature; Rust-built BPF, architecture checking, process-killing default, TSYNC installation | Apache-2.0 OR BSD-3-Clause. Upstream prctl/seccomp FFI contains unsafe. No user-notification supervision API. |
| thiserror =2.0.18 | Typed redacted errors | MIT OR Apache-2.0; proc-macro build dependency. Existing standards approval. |
| dctx-hash / dctx-plugin-protocol | First-party SHA-256 and frozen error codes | Workspace first-party safe Rust. No sha2/Ed25519 production dependency introduced. |

Source locators and reviewed paths:

- [rustix 1.1.5](https://github.com/bytecodealliance/rustix/tree/v1.1.5):
  README.md, Cargo.toml, licence files, src/fs/{openat2,fcntl,memfd_create}.rs,
  src/process/{prctl,rlimit,wait}.rs, src/thread/setns.rs, src/stdio.rs,
  src/runtime_448b8ad740e2a26f.rs. The old safe unshare is deprecated because it
  should be unsafe; no deprecation suppression or wrapper is used. execveat
  takes pointer arrays and requires an unsafe caller. Closing unknown raw FDs
  also requires unsafe: bootstrap refuses any unexpected FD instead of taking
  ownership with FromRawFd or pretending CLOEXEC protects the current callback.
- [Landlock 0.4.7](https://github.com/landlock-lsm/rust-landlock/tree/v0.4.7):
  README.md, CHANGELOG.md, Cargo.toml, src/{ruleset,compat}.rs and sandboxer example.
  ABI9 pathname-Unix mediation, ABI8 all-thread support and ABI7 audit controls
  exist in this version, but the dossier's ABI4 floor is retained. seccomp closes
  later-ABI gaps (ioctl, Unix sockets, signaling, etc.). The primitive does not
  allow open descriptors to substitute for Landlock pathname authorization.
- [seccompiler 0.5.0](https://github.com/rust-vmm/seccompiler/tree/v0.5.0):
  README.md, CHANGELOG.md, Cargo.toml, src/lib.rs, backend/{filter,condition,mod}.rs,
  syscall_table/{x86_64,aarch64}.rs. Syscall tables derive from Linux v6.12.
  apply_filter_all_threads uses TSYNC and reports failures; no empty filter is
  accepted. A static exec allow rule is not one-shot authority and is not added.

## Dependency closure boundaries

Normal Linux dependencies include rustix -> bitflags + linux-raw-sys (or
libc/errno with the alternative backend); landlock -> enumflags2 + libc +
thiserror; seccompiler -> libc. Build/proc-macro edges include enumflags2_derive,
thiserror-impl, syn, quote, proc-macro2 and unicode-ident. Rustix's build script
selects its backend; no C sandbox shim, libseccomp, daemon or installed helper is
added. Rust std, the GNU dynamic loader/libraries and the kernel remain native
trusted code. The trusted probe is dynamically linked and is not evidence that
an external plugin's loader closure has been pinned.

The input workspace lock records bitflags 2.13.2, linux-raw-sys 0.12.1,
libc 0.2.190, thiserror/thiserror-impl 2.0.18, proc-macro2 1.0.107,
quote 1.0.47, unicode-ident 1.0.27 and multiple syn versions. The VM resolved
enumflags2/enumflags2_derive 0.7.12 and compiled syn 2.0.119. This document does
not invent package checksums or describe the input lock as a resolved release graph.
CI owns the refreshed lock; PL-REL must inventory the actual target/feature
closure, licence texts, maintained OS/runtime images and compiler before release.

Dev-only direct pins use the existing lock: tempfile 3.24.0 and proptest 1.12.0.
They do not become library production dependencies. Property failures can persist
inside the owned crate; all disposable file fixtures use crate-local temp roots.

No vulnerability-scan result, full transitive unsafe audit, macOS SDK study or
third-party native launch qualification is claimed by this source review.
