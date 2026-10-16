# macOS confinement profile

PL-R · 2026-10-16 · DX-PLUGIN §4.2 and PL-C S-ADMISSION/PP4.
API feasibility from existing knowledge, without SDK inspection or OS tests.
PL-A/PL-REL consume this; both native architectures remain unsupported.

## Custom sandbox profiles

Seatbelt can apply deny-default rules to filesystem access, network, process
operations, Mach services and IPC. A custom profile is a candidate for the
one-file CLI because a trusted launcher can apply restrictions before executing
the plugin. `sandbox_init`/related entry points are deprecated; the SBPL profile
language and operational behavior are not a stable portable public contract.
The system's `sandbox-exec` utility is also deprecated. Neither is automatically
a production-supported adapter because it happens to exist on the machine.

The profile must start with deny-default and allow only exact immutable
executable/runtime resources and protocol pipes. No recursive read of `/`,
`/Users`, home, project, cache, application support, network, broad Mach service
lookup, distributed notifications, credential services, or unbounded scratch.
Allow only runtime necessities measured on each architecture and OS build.
An `allow file-read*` exception over `/System` is not evidence that all reachable
data is required. Mach-mediated services can perform effects on a process's
behalf; file and TCP denial alone do not establish PP4.

The trusted launcher must sanitize `DYLD_*` and loader search paths before its
own startup, install the profile successfully, close unlisted FDs, and bind the
verified executable before exec. The plugin's dyld initializers must already
be confined. An untrusted worker calling `sandbox_init` in its own `main` is
too late. Sandbox restrictions should persist to descendants, but inheritance
is not a ban on spawning them.

Process policy needs one trusted initial exec and denial of all future plugin
fork/exec/launch-service paths. A path-limited exec allow rule can permit re-exec
of the same artifact; it is not a one-shot launch gate. No reviewed profile/API
combination proving that transition and host-crash reaping is present here.
Process groups, `killpg` and EOF monitoring alone do not contain detachment.
OS denial telemetry available without extra privilege also needs qualification;
do not claim every prohibited attempt produces a host audit receipt.

Direct calls through `libc`/FFI require unsafe Rust. A first-party “small wrapper”
or C shim is not a workaround for `forbid(unsafe_code)`. No exact safe dependency
covering this full profile/launch lifecycle is established in the repository.
See [dependencies.md](dependencies.md). Therefore this path remains blocked,
even before deprecation and per-build conformance concerns are resolved.

## App Sandbox alternative

App Sandbox is an entitlement/code-signing system. It is not a per-child
`Command` option for turning an arbitrary unsigned executable into a precisely
scoped worker. Sandboxing the normal host executable would also restrict its
interactive host/broker operation; the same signed file cannot simply choose
two different entitlement sets by an argv flag.

A separately signed sandboxed helper/XPC service can provide a distinct boundary
inside a desktop bundle. It must omit network client/server entitlements,
user-selected file access, application groups, broad temporary exceptions and
unneeded Mach/IPC privileges. Container storage and inherited handles still
need an inventory; “has App Sandbox entitlement” does not mean zero filesystem
or network authority. Inheritance/signing rules for executing independently
installed binaries, helper IPC and descendant containment require concrete
packaging and adversary evidence on both CPUs.

This alternative may be reasonable for the desktop, but a helper artifact
means the CLI's one-file solution is not established. Do not present embedding
helper bytes then extracting/signing at runtime as an already approved solution.
No code signing, entitlement changes, system helper install or OS bypass occurs
in PL-R. A future provider choice must keep runtime authority narrower than
the plugin grant and preserve no-first-party-unsafe.

## File/launch identity and support gate

macOS does not provide Linux's `openat2`/memfd-sealed-ELF recipe as a drop-in
portable primitive. Handle-relative `openat` and no-follow traversal can narrow
file reads, but pathname execution and dynamic-library loading still need a
namespace that no untrusted writer can replace or modify. Code-signature checks
and hashing followed by pathname `posix_spawn` are not a race-safe launch proof.
`current_exe()` is a path, not a lease on the currently running image.

An administrator-owned distribution directory may provide immutability against
the plugin principal; a user-writable install directory does not automatically
provide immutability against other same-user processes. Document that threat
boundary explicitly. If the caller cannot supply a protected namespace or the
provider cannot safely bind execution, refuse even the bundled launch. decision 080
waives pending worker confinement only, not S-ADMISSION's identity requirement.

Qualification pairs for macOS 14.7.1 and 15.1 on native arm64/x86_64 must include
constructor-time home reads, network/Unix/Mach IPC, dyld injection, symlink and
artifact replacement, fork/re-exec, process detachment and parent death.
All remain unrun. New OS versions require new profile evidence; no generic
“macOS sandboxed” flag or fallback to unrestricted native execution.

Reference locators (not fetched):
[Apple App Sandbox](https://developer.apple.com/documentation/security/app-sandbox),
[entitlements](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.app-sandbox),
the selected SDK's `sandbox.h`, and `sandbox-exec(1)`/`sandbox_init(3)` manpages.
