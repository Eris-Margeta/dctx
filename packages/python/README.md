<!-- SPDX-License-Identifier: Apache-2.0 -->
# dctx for Python

A dependency-free, typed Python 3.11+ transport over `dctx rpc` v1. The engine
produces every output byte. The package never selects context, parses pages,
changes arguments, retries operations, walks sources, caches results or downloads
an engine. See the [RPC contract](../../docs/protocol/README.md) and
[engine philosophy](../../docs/PHILOSOPHY.md).

The wheels are built and tested in CI; **registry publication is pending**.
Nothing is published to PyPI yet, so a `dctx` or `dctx-binary-*` project there is
not from this project: do not install it. Download the matching wrapper and
platform wheel artifacts from CI into `./wheelhouse` (or build them as shown
below), then explicitly install into your project's virtual environment:

```sh
python -m pip install --no-index --find-links ./wheelhouse 'dctx[binary]==1.2.0'
```

The `binary` extra installs only the matching platform distribution, pinned to
the wrapper's exact version. There are no installation hooks or runtime
downloads. Installing the `dctx==1.2.0` wrapper wheel alone is supported when you
already have the matching executable, for example the `dctx` engine from the
[GitHub release archives](https://github.com/Eris-Margeta/dctx/releases) set as
`DCTX_BIN`.

```python
from dctx import Client, EngineError

with Client("./my-project") as engine:
    output: bytes = engine.find("ping", "--limit", "1")
    # Write engine bytes unchanged, including all trailing newlines.
    import sys
    sys.stdout.buffer.write(output)
    try:
        page = engine.read("s:src/lib.rs#ping")
    except EngineError as error:
        # CLI codes: 1=no result, 2=usage refusal, 3=index/source/I/O failure.
        # error.output preserves any stdout emitted before failure.
        sys.stdout.buffer.write(error.output)
        raise
```

Every operation accepts positional CLI argument strings, passed individually
without a shell: `pack`, `context`, `map`, `find`, `read`, `skeleton`, `explain`,
`neighbors`, `grep`, `hubs`, `path`, `languages`, `cache_status`. RPC supplies
`--json` by default. `context` requires `--for TEXT`. `pack` output can include a
summary line after JSON: output is opaque bytes, not a Python object. Budgets,
continuation cursors and paths go straight to the engine. `hello()` returns typed
protocol metadata. Unknown envelope fields and added verbs are tolerated.

**Arguments carry full CLI authority.** Each operation's arguments are CLI argv
for the engine: `--place` selects any readable directory, `pack --output` writes
any path the process may write, and `--clipboard` reaches the clipboard. Never
pass untrusted values (for example text from a model or a web request) as
`--place`/`--output` values or as positionals that start with `-`; validate them
first, and choose the project directory yourself.

Each client owns one synchronous subprocess rooted in the required project
argument. Calls on the same client are serialized; independent projects use
independent clients. Relative operation paths resolve in that project's directory.
Use a context manager or call `close()` to close stdin and reap the process.
`close()` waits for an active call to finish, then allows five seconds for EOF
shutdown before killing. Call `terminate()` from another thread to abort running
work; the session then refuses further operations. There is no automatic timeout
on engine operations. Diagnostics inherit stderr, which avoids an unread stderr
pipe. No daemon or service is started.

The initial hello and every response must report exactly the package's engine
release (`1.2.0`); RPC major version 1 is required independently. A mismatch raises
`VersionMismatchError` and ends the process before any operation is sent. Engine
refusals raise `EngineError` with unchanged `code`, `message`, `hint` and `output`.
Transport failures raise `TransportError`, terminate the session and are never
retried. A missing engine raises `BinaryNotFoundError` with install commands.

Binary resolution is entirely local, in this order:

1. `Client(project, binary="/path/to/dctx")`, otherwise `DCTX_BIN`. An override is
   authoritative; a missing, non-executable or mismatched override fails. Relative
   override paths resolve in the caller's directory, before starting the engine.
2. The installed `dctx_binary_<platform>` module's adjacent `dctx` (`dctx.exe` on
   Windows), provided by the matching platform wheel. The module is accepted
   only from the install directory that holds the imported `dctx` package (and
   its purelib/platlib sibling); `sys.path`, including the working-directory
   entry added by `python -c`, `-m` or the REPL, is not searched. A broken wheel
   fails visibly rather than falling through to another executable.
3. The user cache location returned by `dctx.cache_binary()`. The package never
   creates this directory or fills it. Place an explicitly obtained executable
   here yourself, retaining its executable permission.
4. A hard error. The wrapper never searches `PATH` or downloads anything.

Cache layout: `<cache-root>/dctx/bin/1.2.0/<platform>/dctx`, where cache root is
`$XDG_CACHE_HOME` or `~/.cache` on Linux, `~/Library/Caches` on macOS, and
`%LOCALAPPDATA%` or `~/AppData/Local` on Windows. Windows uses `dctx.exe`.
When no home directory can be determined (for example Windows without
`USERPROFILE` and `LOCALAPPDATA`), the cache step is skipped and the error names
`DCTX_BIN` instead.
Platforms are `linux_x86_64`, `linux_aarch64`, `darwin_x86_64`, `darwin_aarch64`
and `windows_x86_64`. Unsupported hosts can supply a matching engine explicitly.

Build artifacts locally, without publishing:

```sh
cargo build --locked -p dctx-cli
python packages/python/tools/generate_api.py --check
# Wrapper wheel only (also supports PEP 517 / python -m pip wheel).
python packages/python/tools/build_wheels.py --output wheelhouse
# Example development build: distro-specific GNU Linux platform wheel + wrapper.
python packages/python/tools/build_wheels.py --output wheelhouse \
  --binary target/debug/dctx-cli --target x86_64-unknown-linux-gnu
```

The PEP 517 backend has no build requirements, so building the wrapper from its
source distribution works offline too. Wheels have deterministic ZIP metadata,
SHA-256 RECORD entries, `py.typed`, LICENSE and NOTICE. Platform wheels contain
only a module marker, the native engine, and package metadata. The builder
checks VERSION, Cargo and Python versions, then runs an RPC handshake and checks
the executable architecture. It never compiles, downloads or publishes anything.

The `Python packages` workflow builds five native artifacts on their respective
hosts, exercises CLI parity and installs the extra from a local wheelhouse with
network access rejected by a Python audit hook. Its Linux release binaries use
static musl: compressed platform tags admit both manylinux 2.17+ and musllinux
1.2+ hosts. The builder rejects PT_INTERP/DT_NEEDED dependencies for those wheels.
GNU development binaries use honest `linux_*` tags. macOS builds target 10.13
(x86_64) or 11.0 (arm64); Windows uses `win_amd64`. No PyPI credentials or publishing
step are configured.

Run all package checks against a real engine:

```sh
DCTX_TEST_BIN="$PWD/target/debug/dctx-cli" DCTX_REQUIRE_ENGINE_TESTS=1 \
  python -W error::ResourceWarning -m unittest discover -s packages/python/tests -v
```

The tests use the same checked-in corpus and sixteen operation cases as the
Rust RPC parity suite. They compare every verb's exact CLI stdout, error codes
1/2/3 and pre-error stdout. They also check generated types against the schema,
wrapper boundaries, version mismatches, discovery precedence, malformed replies,
threaded serialization, cancellation, reproducible wheel/sdist records, clean
virtual-environment installation and a wrapper-only hard failure. Without
`DCTX_TEST_BIN`, real-engine tests are explicitly skipped; CI requires it.

When releasing, update the Python manifest version, every exact binary-extra
pin, and `src/dctx/_version.py` together with the engine. Regenerate types with
`python packages/python/tools/generate_api.py` after any RPC schema change. Build
and test fail on stale types or release mismatches. Registry publication is a
separate, explicit release action that has not happened yet.
