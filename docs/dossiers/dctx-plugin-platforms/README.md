# Native plugin platform dossier

PL-R · researched 2026-10-16 · repository and existing knowledge only; no network.
Implements DX-PLUGIN §7.1's platform feasibility research for PL-A/PL-G/PL-REL.
Depends on PL-C's frozen admission contract and the Rust/code standards.
This is a design and evidence inventory, not an implemented confinement provider.

## Disposition

**No target has demonstrated third-party native plugin support in this packet.**
All five candidate targets must currently fail third-party enablement with
`UnsupportedPlatform` until their provider, exact dependency closure and native
adversary evidence are accepted. Installing inactive verified bytes, inspecting
metadata and using core pack/map are separate operations. Do not turn a matching
target triple, an OS version check or a signed publisher into execution support.

[PL-C](../../design/plugin-api/README.md) incorporates decision 080: only the official
bundled Git worker authenticated by the containing distribution may use
`bundled-git-pending-confinement`. It still requires consent, selection checks,
brokered effects, audit, race-safe bindings and the separately qualified
[Git recipe](../dctx-plugin-git/recipe.md). An override, installed official
standalone binary or claimed `dctx/git` ID cannot inherit the exception.
The exception does not prove that the worker cannot perform ambient native IO.

decision 081 replaces the earlier proposed production signature dependency with
[first-party verification](../dctx-plugin-trust/README.md). decision 078 keeps Git
behavior in the plugin and explicitly composed brokers, never `dctx-core`.
Neither the older design's all-workers confinement gate nor its earlier crypto
recommendation overrides these decisions.

## Target and version matrix

These are **qualification profiles**, not advertised support or recommended
security patch levels. Exact initial fixture versions make future reproduction
concrete; they are historical baselines, not claims that old patches are safe
to ship in October 2026. PL-REL must pin a maintained patched build, record its
digest/toolchain and repeat qualification on changed inputs. No open-ended
“version or newer” support follows from this table.

| Target | Initial qualification OS/runtime | Mechanism to qualify before external exec | Present disposition |
|---|---|---|---|
| `aarch64-apple-darwin` | macOS 14.7.1 and 15.1, native arm64 | Seatbelt custom profile applied by trusted launcher before loading plugin; App Sandbox alternative requires separate packaging study | Unsupported; safe wrapper/profile/launch evidence absent |
| `x86_64-apple-darwin` | macOS 14.7.1 and 15.1, native Intel | Same, tested independently of arm64 | Unsupported; Rosetta is not evidence |
| `x86_64-unknown-linux-gnu` | Ubuntu 24.04, kernel `6.8.0-31-generic`, glibc `2.39`; Landlock ABI >=4 probed | User/mount/PID/network/IPC namespaces, Landlock, seccomp, supervised launch | Unsupported; safe launch/namespace composition unproven |
| `aarch64-unknown-linux-gnu` | Ubuntu 24.04, kernel `6.8.0-31-generic`, glibc `2.39`; native arm64 | Same with independent syscall/loader rules | Unsupported; x86 results do not transfer |
| `x86_64-pc-windows-msvc` | Windows 11 23H2, build family `22631`, initial fixture `22631.4169`; native x64 | AppContainer with no network capabilities, restricted ACLs, suspended creation and nonbreakaway job | Unsupported; no approved safe creation/binding wrapper |

No Git is required for pack/map or authorized edit-only protection. The narrow
Git qualification pin is upstream **2.47.0** on Unix and **Git for Windows
2.47.0.windows.1** on Windows; neither is a current security recommendation or
an accepted runtime here. See the Git dossier for artifact and feature gates.

macOS before the named profiles, Windows 10/Server/ARM64, Linux kernels without
the probed features, musl/Android/BSD, WSL1/WSL2 and containers without the
required namespace permissions are unqualified. They return
`UnsupportedPlatform`; no shell wrapper, host emulation or unrestricted fallback.
WSL2 could become a separately qualified Linux environment, not implicit support.
Artifact `min_os`/glibc fields describe compatibility floors; the host also needs
a tested provider profile and current release allowlist.

## One-binary feasibility

“One binary” means one dctx CLI executable containing the bundled Git worker's
Rust code and manifest. It does **not** mean embedding Git itself, statically
linking the OS, eliminating system libraries, or packaging independently
installed plugins into that file. Missing Git leaves edit-only behavior.

| Target | Same-executable bundled worker | Confined third-party mechanism and packaging cost |
|---|---|---|
| Linux x86_64 / aarch64 | Feasible structurally; trusted launcher entry before normal startup | Conditional: linked safe adapters and supervisor can fit one executable; external `bwrap`/`unshare` helper would add a dependency and is not the selected solution. No privileged helper assumed. |
| macOS arm64 / x86_64 | Feasible for the decision 080 pending-confinement mode | Custom Seatbelt path could preserve one executable if a reviewed safe API and supported profiles exist. App Sandbox usually entails separately entitled helper/app packaging; it does not establish a one-file CLI solution. |
| Windows x64 | Feasible for the decision 080 pending-confinement mode | AppContainer/job APIs can structurally be linked into one `.exe`; safe wrapper and DLL closure still block qualification. A separate service/helper is not silently permitted. |

The same executable's early launcher is trusted host code, not plugin code.
It must authenticate its private bootstrap channel before any user command,
configuration, GUI initialization or plugin instruction. A command-line worker
flag is not authority. For an external binary, constructors and the dynamic
loader are part of execution: restriction after `main` or after hello is late.
Desktop bundles may contain a signed helper only with a disclosed inventory;
that does not satisfy the single-file CLI promise by itself.

## Reading map and acceptance

| Dossier | Decision supplied |
|---|---|
| [linux.md](linux.md) | Layered kernel controls, pre-exec ordering, one-shot launch problem |
| [macos.md](macos.md) | Seatbelt versus App Sandbox, process/network limits and safe API gap |
| [windows.md](windows.md) | AppContainer versus job objects, suspended launch and ACL limits |
| [bindings.md](bindings.md) | Race-safe reads, conditional replacement and executable identity |
| [dependencies.md](dependencies.md) | Exact candidate pins, licences, known unsafe/native boundary and unresolved closure |
| [../dctx-plugin-trust/](../dctx-plugin-trust/README.md) | TLS, decision 081 verifier, signing and key manager |
| [../dctx-plugin-git/](../dctx-plugin-git/README.md) | Supported Git subset, configuration/attributes and tests |

PL-A must record for each accepted OS build: kernel/OS and CPU, Rust toolchain,
provider revision/profile digest, resolved target/features/native dependency
inventory, executable/runtime digests, effective restrictions, and a successful
positive invocation next to each escape refusal. Include direct project/home/
cache access, IPv4/IPv6/UDP/Unix IPC, inherited handles, symlink/reparse races,
replacement during verification/launch, constructors before hello, fork/exec,
detachment, cancellation, parent crash and malicious Git helper attempts.
No timing-only assertion substitutes for observing the forbidden effect.

Audit the actual OS denials where feasible. Some kernel denials are observable
only as an error inside an untrusted process; a plugin can suppress that error.
If the §4.2 promise to audit/quarantine every attempted escape needs privileged
tracing or mediation, record that as another unmet provider property. Do not
infer attempted syscalls from a cooperative worker's messages.

This packet runs only its three supplied directory predicates once, after all
edits. It runs no Cargo, Git subprocess fixtures, downloaded code, network
research or OS experiments. Directory existence is not runtime evidence.
PL-A/PL-G/PL-I/PL-REL supply their authorized evidence; PL-V owns the final
once-only remote workspace suite. No dependencies are approved or added here.
