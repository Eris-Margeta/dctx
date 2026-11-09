<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx

A dependency-free Node.js transport for `dctx rpc`. The native engine produces
all context, maps, results and diagnostics. This package forwards argument
strings and returns engine envelopes without parsing or rendering their output.
It includes the `dctx` CLI command, which verifies the engine handshake and then
forwards CLI arguments, standard streams, exit status and signals.

Install from npm (Node 18+):

```sh
npm install dctx
npx dctx --version
```

The `dctx` package pins `@tejl/dctx-<os>-<arch>` optional dependencies, and npm
installs the matching native platform package. If optional dependencies were
omitted, install the package for your platform at the same version, for example
`npm install @tejl/dctx-linux-x64@1.3.1`.
The transport itself makes no network calls and has no install hooks, binary
downloads, fallback operations or retries.

Install a CI tarball offline-safe, then point it at a verified release binary:

```sh
npm install --omit=optional --ignore-scripts ./dctx-1.3.1.tgz
# The matching archive from the GitHub release, checked against its SHA256SUMS.
export DCTX_BIN=/absolute/path/to/dctx
```

The CI tarball (`packaging/npm/scripts/build-pack.mjs`) declares no
dependencies, so npm resolves nothing from a registry for it. Registry builds
(`tools/release/prepare-npm.py`) declare the native platform packages
`@tejl/dctx-<os>-<arch>` as pinned optional dependencies. To install prepared
tarballs without resolving optional dependencies from a registry, name the
matching platform tarball explicitly:

```sh
npm install --omit=optional --ignore-scripts ./dctx-1.3.1.tgz ./tejl-dctx-linux-x64-1.3.1.tgz
```

Registry installs normally include optional dependencies. For an offline project,
keep `omit=optional` in its `.npmrc` so later installs also skip registry lookups.

```js
import { connect } from 'dctx'; // In an ES module (.mjs)

const engine = await connect({ cwd: '/path/to/project' });
try {
  const reply = await engine.request('read', ['m:src/lib.rs', '--lines', '1-20']);
  if (reply.error) {
    // The engine owns code, message, hint and any output produced before failure.
    process.stdout.write(reply.error.output);
    process.exitCode = reply.error.code;
  } else {
    process.stdout.write(reply.result.output);
  }
} finally {
  await engine.close();
}
```

CommonJS also exposes `const { connect } = require('dctx')`; use it inside an
async function.

`request(verb, args)` supports every verb in the
[RPC v1 schema](https://github.com/Eris-Margeta/dctx/blob/main/docs/protocol/dctx-rpc-v1.schema.json),
except the initial `hello`, which `connect()` sends automatically. `args` consists
of CLI argument strings, with no shell splitting or interpolation. The engine
selects JSON output. `result.output` and `error.output` are opaque UTF-8 strings,
including all trailing newlines; they need not contain a single JSON document.
Engine errors resolve as their original envelopes, including unknown fields.
Transport failures reject. The client never reconstructs CLI diagnostics.

**`args` carry full CLI authority.** They are CLI argv for the engine: `--place`
selects any readable directory, `pack --output` writes any path the process may
write, and `--clipboard` reaches the clipboard. Never pass untrusted values (for
example text from a model or a web request) as `--place`/`--output` values or as
positionals that start with `-`; validate them first, and choose `cwd` yourself.

`connect({ binaryPath?, cwd?, env?, onStderr?, handshakeTimeoutMs? })` requires
`engine_version` to equal this package's version exactly and protocol major 1.
The handshake deadline defaults to 10 seconds. Operations have no transport
budget or timeout. `env` replaces the child environment; it defaults to
`process.env`. `onStderr` receives raw `Uint8Array` chunks; diagnostics otherwise
forward to `process.stderr`. `engine.hello` exposes immutable handshake metadata.

Multiple submitted requests retain FIFO order and have unique string IDs.
`close()` closes stdin, drains submitted requests and waits for exit. `abort()`
terminates the process, rejects pending requests and waits for exit; no result
is promised for aborted work. A new connection starts a new engine process.
There is no daemon or in-band cancellation.

Binary resolution, also exposed as `resolveBinary(options)`, stops at the first
available source:

1. `binaryPath`, then `DCTX_BIN`, as an executable path. Relative paths resolve
   against `cwd`. An invalid explicit choice fails immediately. The npm command
   shim itself is refused to prevent recursive process startup.
2. The pinned optional package `@tejl/dctx-<os>-<arch>`, containing `bin/dctx`
   (`bin/dctx.exe` on Windows), only when this package's manifest declares it
   (registry builds; the CI tarball does not). Linux x64/arm64, macOS x64/arm64
   and Windows x64 are packaged. An installed binary that cannot be executed
   fails immediately.
3. A binary placed manually in the per-user cache. `cacheBinaryPath({ env? })`
   returns the exact path: `<cache>/dctx/npm/<version>/<os>-<arch>/dctx`, with
   `.exe` on Windows. `<cache>` is absolute `XDG_CACHE_HOME` or `~/.cache` on
   Linux, `~/Library/Caches` on macOS, and `LOCALAPPDATA` or
   `~/AppData/Local` on Windows.
4. An error with the release URL, checksum verification instruction and cache
   destination, plus, when the platform package is declared, the
   `npm install @tejl/dctx-<os>-<arch>@<version>` command. The transport never
   downloads a binary itself.

To use your own engine, obtain the matching release, verify `SHA256SUMS`, and
set `DCTX_BIN` to its executable path (or use `binaryPath`). On Unix, give a
manually placed binary executable permission with `chmod 755`. PATH lookup is
intentionally absent. Every connection verifies the selected binary's version.
TypeScript declarations are generated from and checked against the committed
RPC schema; the Node transport has no runtime npm dependencies.
