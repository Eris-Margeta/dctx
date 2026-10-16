# dctx-plugin-platform (PL-Ab)

Safe Rust platform primitives for PLUGINS §4.2/§7.2 PL-A and the frozen
S-ADMISSION, S-FILE, S-ARTIFACT, PP2/4/7/8/9 contracts. **No target is advertised
as a qualified native plugin launcher.** Unsupported capabilities return a typed
`UnsupportedPlatform`; no path is retried with an unrestricted `Command`.

## Implemented boundary

`linux::run_restricted` is a terminating entry for **already-loaded trusted code**
in a disposable single-threaded process. It never loads an arbitrary plugin.
It checks the actual environment, three pipe descriptors and absence of extra
descriptors/threads, binds parent death with a before/after parent-ID check, sets
no-new-privileges, disables dumps and file growth, bounds address space, changes
cwd away from the project, installs a strict Landlock ruleset and finally a
seccomp allowlist. Failure exits 125 before the callback. The callback cannot
return to an ambient host: its result exits the process.

Landlock handles all ABI4 filesystem rights and TCP bind/connect with no allows;
HardRequirement and FullyEnforced are mandatory. Seccomp independently denies
path opens/metadata, sockets (including UDP/IPv6/Unix), exec/execveat, fork/vfork/
clone/clone3, setsid, tracing, io_uring, namespace operations, FD duplication,
privilege/lifecycle changes and all other unlisted syscalls. It allows pipe IO
only on the appropriate stdio FDs and anonymous private non-executable memory.
Architecture checking is compiled by seccompiler; there is no x32 syscall escape.
Violations terminate with SIGSYS, which the future host must quarantine/audit.
Successful setup is not a release qualification token or broker grant.

`RootHandle` uses `openat2(BENEATH|NO_SYMLINKS|NO_MAGICLINKS|NO_XDEV)` relative to
a host-opened root. It refuses hardlinks, nonregular objects and ambiguous names.
Metadata preconditions are **not** immutable content versions. `snapshot` checks
the exact host-authorized digest and length of the copied bytes with dctx-hash,
revalidates object identity, and publishes only a sealed memfd. Seals prohibit
write/grow/shrink and further seal changes. Reads are bounded to 1 MiB/chunk and
copies to 64 MiB. The broker checks selection/consent before every open/hash and
revalidates policy before delivery; this library has no policy authority.

`NoSecretEnvironment` clears all ambient/configured variables and supplies only
`LANG=C`, `LC_ALL=C`. The private entry also rejects unexpected actual variables.
There is no inherited HOME/PATH/loader/Git/token allowlist.

## Explicit unsupported capabilities

| Operation | Disposition and reason |
|---|---|
| Linux third-party ELF launch | Unsupported: safe one-shot exec plus private namespaces/runtime view are absent. A sealed memfd alone cannot authorize repeated exec or bind an ELF interpreter. |
| Bundled Git launch | Unsupported: decision 080 waives pending confinement only; it does not waive verified image/runtime binding, authenticated bootstrap and complete lifecycle evidence. The restriction callback is a building block for that trusted entry. |
| Brokered Git recipe | Unsupported: no claim of qualified Git versions/configuration/attributes or enforced repository writer exclusion. |
| Existing-file conditional replace | Unsupported before any write: renameat2 and advisory locks cannot atomically compare identity/content against a hostile writer. |
| macOS arm64/x86-64 | Unsupported: safe Seatbelt/initial-exec/image-binding/lifecycle composition is absent. See the profile notes below. |
| Windows and other targets | Unsupported; no raw FFI, helper or native emulation fallback. |

The current approved rustix `unshare_unsafe`, runtime `kernel_fork`, `execve` and
`execveat` require unsafe callers. Its deprecated safe `unshare` is explicitly
documented upstream as unsound and is not used. seccompiler provides filter
installation, not a safe user-notification one-shot supervisor. See
[DEPENDENCIES.md](DEPENDENCIES.md) for source-reviewed details and closure limits.

## macOS profile path

`profiles/macos-deny-all.sb` is a deliberately non-runnable qualification baseline:
version 1, deny default, no filesystem/process/network/Mach/service exceptions.
`macos::DENY_ALL_PROFILE` embeds it; Linux tests verify its exact deny-default
contents and refusal disposition. No `sandbox-exec` or `sandbox_init` invocation
is implemented. A future reviewed safe provider must apply a pinned profile
before dyld/plugin constructors, permit only measured immutable runtime resources
and pipes, deny descendants/Mach-mediated effects, and prove image binding and
parent-crash reaping on each native CPU/OS build. The Linux tests are not evidence
that this profile works on macOS.

## Validation and limits

The packet's remote `cargo test -p dctx-plugin-platform` exercises real child
processes via the trusted `dctx-platform-probe` fixture. This binary is test tooling,
not a plugin launcher and not part of the default dctx distribution. Positive
restricted computation precedes direct source/fake-home/fake-cache reads,
IPv4/IPv6/UDP/Unix access, write/metadata/cwd, spawn/exec/Git descendants, threads,
detachment and disabling parent-death handling. A three-process fixture uses its
own subreaper to prove that a computing worker dies and is reaped when its parent
crashes; EOF cooperation cannot explain success. A separate process demonstrates
Landlock denial without seccomp masking it. Tests report the actual kernel/CPU
or explicit lack of kernel support directly in command output.

Filesystem tests cover sealed-image mutation/path replacement, changed digests,
hardlinks, leaf/ancestor symlinks, repeated concurrent symlink substitution,
retained-root renaming, overflow and refused replacement after removal.
Property tests cover path traversal. Fixtures live inside this crate and are
removed by RAII; no developer home, real Git configuration or source is used.

The tests are evidence only for the observed kernel and trusted entry. They do
not qualify arbitrary ELF constructors, runtime/loader substitution, private
mount/PID/network namespaces, the complete host admission/audit transaction,
Git administrative effects or other CPUs/OS builds. Full support remains closed.
If Landlock/seccomp is unavailable, tests record **UNQUALIFIED KERNEL**, exercise
the refusal, and do not claim successful confinement from a green suite alone.

All first-party targets forbid unsafe code. Only the supplied remote Cargo
commands are used for builds/tests/lint; root Cargo.toml and Cargo.lock are not
edited by this packet. Shared lock refresh and release SBOM belong to CI/PL-REL.
