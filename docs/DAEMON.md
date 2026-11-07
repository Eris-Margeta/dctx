# Optional local daemon

The daemon is disabled by default. Enable auto-start for read-only `map`, `read`
and stdout `pack` commands with `DCTX_DAEMON=1`, or persist the preference with:

```sh
dctx daemon config on
dctx daemon status
dctx daemon stop
dctx daemon config off
```

`DCTX_DAEMON=0` overrides the saved preference. Configuration is stored in
`daemon/config.json` under the normal dctx cache root (`DCTX_CACHE_DIR` overrides
that root). `status` prints JSON, does not start a daemon and reports its PID and
engine version when running. `stop` is idempotent. Disabling auto-start lets an
existing daemon reach its idle timeout; use `stop` to end it immediately.

The first enabled query starts the same executable as a background process. The
transport returns the existing engine transaction's stdout, failure and exit code.
If startup, permissions, connection, framing or response delivery fails, the CLI
runs the original in-process transaction before emitting any output. These
transactions are read-only and can safely be retried. File output, clipboard operations, RPC/MCP sessions and profiling/reuse diagnostics continue
using their existing in-process paths.

The CLI runs the usual auto-clean slice before sending the query. It forwards
that maintenance choice to the engine session, including `--no-auto-clean`, so
maintenance diagnostics still use the caller's stderr and the slice is not run twice.

Each request opens a fresh engine session. A process-local cache retains one
decoded index (at most 4 MiB encoded and 50,000 total rows), avoiding repeated
index decoding. Larger indexes still work through the usual engine path. Full
retained-object equality and the decoder's record ceiling key this cache. All
source freshness, ignore rules, policy, verification, cache admission and lease
checks still run; the daemon does not keep rendered answers or skip source checks.

The handshake matches the daemon protocol, package version, build revision and executable fingerprint (path, size and
modification timestamp).
It also matches the working directory and inherited environment, so changes to
freshness, limits, ignore settings or other process settings restart the daemon.
Only `DCTX_DAEMON` is excluded from the environment identity. A mismatch asks the
old daemon to exit and starts the current executable; unavailable restarts fall
back in-process. Identities are hashes; environment contents are not sent or
stored in daemon metadata. Processes using different environments may restart the
shared daemon, affecting reuse but not correctness.

The idle timeout defaults to 300 seconds. Set `DCTX_DAEMON_IDLE_SECONDS` to an
integer from 1 through 86400. The timer starts at startup and resets after a
connection completes, so a running query is not killed by the idle timer. Frame
reads are bounded to 1 MiB for requests and 128 MiB for responses; handshake I/O
has a two-second timeout, and client command I/O a sixty-second timeout. A client
that disconnects cannot make the daemon emit a partial answer to a later client.

Unix uses a mode-0600 Unix socket in a user-owned mode-0700 directory. Unsafe
existing directories and endpoint types are refused. An OS file lock serializes
listeners and recovers stale sockets after a crash. Windows uses a byte-mode
named pipe with remote clients rejected, a protected DACL granting access only to
the process token's user SID, a verified server owner, and no client impersonation
rights. The server accepts one request at a time. No platform opens a network
listener. The daemon and its decoded index disappear at idle exit or `stop`;
persisted indexes remain governed by the normal dctx cache policy.

No `doctor` command exists in the base CLI for this change. `dctx daemon status`
provides the daemon's read-only diagnostic entry point.

The Windows implementation follows the platform's [named pipe wait-mode
semantics](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-type-read-and-wait-modes).
Clients acknowledge complete responses before server-side handle shutdown,
keeping [pipe shutdown](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-operations)
bounded without an indefinitely blocking flush.
