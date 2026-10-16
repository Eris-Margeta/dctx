# PL-Ab validation — 2026-10-16

Remote machine `the host project`, native x86_64, Linux
`6.12.111+deb13-cloud-amd64`. These are primitive/trusted-entry results, not a
third-party launch qualification. NativeProvider continues to refuse all launch
kinds on every target; the README lists the unmet guarantees.

| Predicate | Result |
|---|---|
| `test -s crates/dctx-plugin-platform/src/lib.rs` | Passed |
| `cargo test -p dctx-plugin-platform` | Passed: 9 unit/property + 5 binding + 3 subprocess tests; zero ignored/failed |
| `cargo clippy -p dctx-plugin-platform -- -D warnings` | Passed |

The successful suite emitted actual evidence, not its unsupported-kernel branch:

```text
PL-Ab Landlock ABI4 denial observed independently of seccomp
PL-Ab native evidence: computation, 3 direct reads, 14 syscall adversaries, parent crash/reap; arch=x86_64 kernel=6.12.111+deb13-cloud-amd64
```

The restricted computation read `ping`, allocated/computed privately and returned
`pong` over pipes. Direct source/fake-home/fake-cache reads and the 14 forbidden
operations terminated with SIGSYS, without forbidden output or marker writes.
The parent-crash fixture observed SIGKILL and reaped the computing child via its
own subreaper. Landlock denial was separately observed as PermissionDenied after
a successful read of the same private fixture before restriction.

Binding tests observed immutable verified bytes after source inode/path changes,
and refused hardlinks, symlinks, concurrent substitution and unsupported
conditional replacement. Bootstrap refused extra descriptors, secret environment,
non-pipe stdin, extra threads and mismatched parent before entering the body.

Two earlier test invocations stopped during compilation; no suite ran in either.
Worker-layer causes were the rustix Pid conversion method name and proptest's
diagnostic expansion around interpolated format strings. Both were corrected
before rerunning with changed inputs. There was one completed full suite, at the
end; no unchanged validation rerun or local Cargo invocation occurred. Rustfmt
formatted only owned source files. No root manifest/lock or generated governance
file was edited.

The remote runner reported that it could not append its external telemetry log
under the local state directory (EPERM); build/test/lint command outputs and exit statuses
were returned normally. The crate-local opensrc research cache remains ignored
and excluded from packaging because automatic policy review rejected deletion:
`effect-not-in-packet,outside-requires-done-and-grant`. Orchestrator cleanup is
needed; PL-Ab did not retry the denied deletion through another mechanism.
