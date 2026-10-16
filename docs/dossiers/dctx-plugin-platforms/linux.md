# Linux confinement profile

PL-R · 2026-10-16 · DX-PLUGIN §4.2 and PL-C S-ADMISSION/PP4.
Design from kernel API knowledge; no kernel/source probes or native tests here.
PL-A consumes this with dependencies.md and bindings.md; status: unsupported.

## Required composition

Landlock alone is insufficient. Its early filesystem ABIs do not cover every
operation; ABI 4's network mediation covers TCP bind/connect, not all networking.
It neither creates a private filesystem view nor prevents fork/exec. seccomp
alone cannot safely interpret pathname pointers. Namespaces alone do not turn
all host-visible mounts and inherited descriptors into a capability boundary.

Initial qualification uses Linux 6.8 with Landlock ABI 4 or a later probed ABI
whose semantics have their own evidence. Relevant floors: Landlock appeared in
5.13; ABI 2 adds REFER (5.19), ABI 3 TRUNCATE (6.2), ABI 4 TCP mediation (6.7).
Handle every required access right supported by the chosen ABI; never silently
accept partial/best-effort restriction. Probe seccomp and user/mount/PID/network
namespace creation as well. Distribution policies may disable unprivileged
user namespaces or Landlock despite a sufficient kernel version.

## Trusted launch ordering

1. Parent validates consent, target, immutable artifact/runtime handles and
   audit availability. Create bounded protocol pipes and authenticated bootstrap.
   Do not give the worker root paths, a mount-control FD, network sockets, secret
   environment, the audit sink or the launch supervisor's control descriptor.
2. Start a trusted, minimal, single-threaded launcher entry. This avoids a
   first-party `unsafe CommandExt::pre_exec` closure and post-fork Rust allocator
   hazards. Starting this trusted code before restrictions is not starting an
   external plugin. The dependency providing namespace/launch primitives must
   expose reviewed safe APIs; copying raw syscalls into dctx is forbidden.
3. Establish user IDs without ambient host capabilities; deny supplementary
   group changes, create private mount, network, IPC and PID namespaces, make
   mount propagation private. Set up a new minimal root and discard old-root
   access. A privileged process in a new user namespace is not host root.
   Reject setup failures; do not retry without any of these layers.
4. Mount only pinned executable/loader/library resources read-only and bounded
   private scratch if approved. No project, home, cache, host `/proc`, devices,
   system-wide `/tmp`, credentials or writable dynamic-loader search roots.
   Use an empty working directory. Supply only required minimal device access,
   if any, with an explicit per-node inventory. Namespace init/supervisor owns
   reaping; the plugin is not PID 1 with uncontrolled lifecycle semantics.
5. Set `no_new_privs`, remove capabilities, set resource ceilings, apply Landlock
   to every required filesystem right with minimal read/execute exceptions.
   Landlock restrictions inherit across exec/fork but pre-opened descriptors
   can retain authority: close all unlisted descriptors before admission.
6. Apply an architecture-specific seccomp allowlist before the external ELF
   loader/constructors run, and atomically bind the allowed initial execution
   to the verified artifact. Only then may a worker instruction or hello run.
   Parent admits broker traffic only after trusted setup success.

This sequence specifies obligations, not a ready-to-use Rust API recipe. User
namespace mapping, process creation and execution wrappers are unresolved in
the exact approved dependency set. PL-A must close that gap before implementing
a provider; `rustix`, Landlock and seccompiler names alone do not close it.

## Initial exec versus future exec

A static seccomp filter allowing `execve` for launch also allows later exec.
Pathname comparison in a filter is unavailable. Allowing `execveat` on one FD
does not make that FD number an immutable authority: reuse and re-exec matter.
Do not promise “no descendants” on the strength of that filter.

A possible provider uses seccomp user notification for a **single** supervisor-
authorized `execveat` on the sealed executable descriptor, then refuses every
later exec. The launcher is trusted at the initial notification; subsequent
requests from plugin code are never continued. Deny clone/fork/vfork, descriptor
replacement and tracing while this transition occurs. Never use notification
to authorize mutable pathname memory supplied by untrusted code: `CONTINUE`
has a TOCTOU hazard. A precise safe wrapper, state transition proof, FD lifetime
proof and crash behavior are still missing; this is not implemented by merely
calling seccompiler's filter installation API.

The bundled Rust worker could enter its own code after final restriction in
the trusted launcher without a second exec. That narrower path does not solve
arbitrary third-party ELF launch. A runtime library constructor cannot be
trusted to install a filter on behalf of an arbitrary third-party binary.

## Network, process and lifecycle rules

- No host network interfaces/routes or host network namespace handles. Drop
  loopback connectivity as well; deny socket/connect/bind/listen and relevant
  send/receive operations, including IPv6, UDP, raw and Unix-domain channels.
  Stdio uses pipes. Network namespaces do not isolate filesystem Unix sockets
  that remain mounted; the filesystem policy must exclude them.
- Deny exec after the one allowed launch; deny clone/clone3/fork/vfork in the
  initial single-threaded artifact profile. Multithreaded runtimes require a
  separately qualified clone-flag policy; `clone3` points to a struct that classic
  BPF cannot inspect. Return unsupported-runtime instead of broad clone access.
- Deny ptrace, process_vm operations, mount/setns/unshare, BPF, perf, keyrings,
  io_uring and other unneeded interfaces. Do not share an abstract Unix socket
  namespace or inherited authenticated IPC with the host.
- Use a PID namespace supervisor and parent-death/pipe monitoring whose behavior
  is tested if either parent or init crashes. Process groups alone can be escaped
  by `setsid`. A cgroup v2 subtree with `pids.max`/memory/kill is useful only when
  explicitly delegated; do not assume a user can administer host cgroups.
- Resources are bounded by policy, and cancellation closes broker admission
  before termination. Reap all children, retaining `reap_pending` if cleanup is
  unconfirmed. `kill` success is not a reap receipt.

## Binding and packaging

Candidate launch binding: copy from an authorized verified file into a private
memfd, hash the actual copy, apply write/grow/shrink/seal seals, then exec the
sealed object with `execveat(AT_EMPTY_PATH)`. Enforce executable-memfd policy on
the selected kernel; reject scripts and interpreter fallback. Holding a plain
read-only FD alone does not prevent another writer from modifying its inode.
Pin loader/shared-library bytes too. Linux GNU artifacts need their exact glibc
ABI/runtime inventory, not an assumed fully static binary.

This can fit the one-dctx-executable shape if safe dependency adapters provide
all primitives. External bubblewrap/util-linux tools, setuid helpers and daemon
services are not included or approved. Third-party execution remains disabled
while any launch, cleanup, safe-wrapper or native evidence obligation is open.

Reference locators (not fetched):
[Landlock ABI](https://docs.kernel.org/userspace-api/landlock.html),
[seccomp](https://docs.kernel.org/userspace-api/seccomp_filter.html),
[seccomp notification](https://man7.org/linux/man-pages/man2/seccomp.2.html),
[openat2](https://man7.org/linux/man-pages/man2/openat2.2.html),
[memfd](https://man7.org/linux/man-pages/man2/memfd_create.2.html).
