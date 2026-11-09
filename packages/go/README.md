<!-- SPDX-License-Identifier: Apache-2.0 -->
# dctx for Go

Module `github.com/Eris-Margeta/dctx/packages/go` is a Go 1.23+ transport over
`dctx rpc` v1, using only the standard library. The engine produces every output
byte. Operations accept CLI argument strings and return opaque `[]byte`, including
all trailing newlines. See the [RPC contract](../../docs/protocol/README.md) and
[engine philosophy](../../docs/PHILOSOPHY.md).

The module is built and tested in CI; **publication is pending**. Its module tag
does not exist yet, so `go get` of this module path does not work today. Use it
from a checkout with a `replace` directive (below), and get the engine from the
[GitHub release archives](https://github.com/Eris-Margeta/dctx/releases).

```go
package main

import (
    "context"
    "errors"
    "log"
    "os"

    "github.com/Eris-Margeta/dctx/packages/go"
)

func main() {
    ctx := context.Background()
    engine, err := dctx.New(ctx, "./my-project", dctx.Options{})
    if err != nil {
        log.Fatal(err)
    }
    defer engine.Close()

    output, err := engine.Find(ctx, "ping", "--limit", "1")
    if err != nil {
        var failure *dctx.EngineError
        if errors.As(err, &failure) {
            // Preserve any CLI stdout emitted before the error.
            _, _ = os.Stdout.WriteString(failure.Output)
        }
        log.Print(err)
        return
    }
    _, _ = os.Stdout.Write(output)
}
```

The generated methods are `Pack`, `Context`, `Map`, `Find`, `Read`, `Skeleton`,
`Explain`, `Neighbors`, `Grep`, `Hubs`, `Path`, `Languages`, and `CacheStatus`.
`Call(ctx, dctx.VerbFind, args...)` provides the same transport directly. `Hello`
returns typed protocol metadata. Arguments pass individually without a shell;
RPC supplies `--json`. Pass `"--for", "your task"` to `Context`. Budget flags,
cursors, paths and output destinations retain engine semantics. Pack output can
include a report line after JSON; it remains opaque bytes.

**Arguments carry full CLI authority.** They are CLI argv for the engine: `--place`
selects any readable directory, `pack --output` writes any path the process may
write, and `--clipboard` reaches the clipboard. Never pass untrusted values (for
example text from a model or a web request) as `--place`/`--output` values or as
positionals that start with `-`; validate them first, and choose the project
directory yourself.

Each client owns a subprocess in the required project directory. Calls are
serialized; independent clients are independent engine sessions. Queued calls
can be canceled without affecting active work. Canceling an **active** call kills
and reaps the engine and ends that session, because RPC v1 has no in-band
cancellation. The canceled call returns its context error. No operation is
retried. The context passed to `New` controls startup and hello only; use a context
on each subsequent call to bound that operation.

`Close` waits for active work, sends EOF, and allows five seconds for engine
shutdown before terminating. It returns a transport error for an unexpected
nonzero process exit. `Terminate` can abort active work from another goroutine.
Both are safe to call repeatedly. Diagnostics inherit stderr; no diagnostic pipe
can fill unnoticed. Responses have no wrapper-imposed output-size limit.

`EngineError` preserves `Code`, `Message`, `Hint` and `Output`. Branch on `Code`:
1 means no result, 2 usage/protocol refusal, and 3 index/source/I/O failure (the
legacy cache command retains its CLI code 1). Engine refusals leave the session
usable. `TransportError` and `VersionMismatchError` terminate it. The initial
hello and every response must report engine **1.2.0**; RPC major **1** is checked
independently. Unknown envelope fields and added verbs are tolerated.

Binary discovery is entirely local:

1. `Options{Binary: "/absolute/path/to/dctx"}`, otherwise the caller's `DCTX_BIN`.
   Overrides are authoritative, including a configured empty `DCTX_BIN`. Relative
   overrides resolve in the caller's current directory before engine startup.
2. `CacheBinary()`, a versioned per-user path. The package creates neither the
   directory nor its contents.
3. A hard `BinaryNotFoundError` with exact installation instructions. Discovery
   never searches `PATH`, falls back from an unusable override, or downloads.

The cache layout matches the Python package:
`<cache-root>/dctx/bin/1.2.0/<platform>/dctx` (`dctx.exe` on Windows).
Cache roots are `$XDG_CACHE_HOME` or `~/.cache` on Linux, `~/Library/Caches` on
macOS, and `%LOCALAPPDATA%` or `~/AppData/Local` on Windows. Platforms include
`linux_x86_64`, `linux_aarch64`, `darwin_x86_64`, `darwin_aarch64`, and
`windows_x86_64`. Other hosts can provide a matching engine explicitly.

Download the engine explicitly from
[v1.2.0 releases](https://github.com/Eris-Margeta/dctx/releases/tag/v1.2.0), verify
the archive against `SHA256SUMS`, extract it and set `DCTX_BIN` to its absolute
executable path, or place it at `CacheBinary()`. A tagged source install is also
explicit:

```sh
cargo install --locked --git https://github.com/Eris-Margeta/dctx --tag v1.2.0 dctx-cli
export DCTX_BIN="$HOME/.cargo/bin/dctx-cli"
```

On Windows, set `$env:DCTX_BIN` to the installed `dctx-cli.exe` absolute path.
There are no installation hooks, bundled executables or runtime downloads.

Until a module tag exists, use a local replacement in the consuming module's
`go.mod`:

```text
require github.com/Eris-Margeta/dctx/packages/go v1.2.0
replace github.com/Eris-Margeta/dctx/packages/go => ../dctx/packages/go
```

Run from the repository root:

```sh
cargo build --locked -p dctx-cli
go -C packages/go generate ./...
go -C packages/go run ./internal/generate -check
go -C packages/go vet ./...
DCTX_TEST_BIN="$PWD/target/debug/dctx-cli" DCTX_REQUIRE_ENGINE_TESTS=1 \
  go -C packages/go test -race -count=1 -timeout 3m -v ./...
```

Types, verb constants, operation methods and the pinned release come from the
committed RPC schema and `VERSION`. Generation has no dependencies or network
access. Tests check the schema fingerprint, CLI manifest version, wrapper
capabilities, discovery precedence, large opaque output, malformed envelopes,
version mismatches, concurrent correlation, cancellation and shutdown. Real
engine tests run the same sixteen operation cases as the Rust RPC suite, plus
error parity. A missing engine fails locally. Only CI jobs without a binary may
skip with an explicit message; the Go workflow requires a binary and forbids
that skip. Linux tests both Go 1.23 and stable; macOS and Windows test stable.

Release preparation consists of updating the engine version, regenerating and
checking the API, and passing the gates. The module's eventual Git tag must be
`packages/go/v1.2.0`, matching Go's subdirectory-module convention. Only after
that tag is pushed will `go get` of the module resolve; no such tag has been
created yet, and no publishing or registry upload step is configured.
