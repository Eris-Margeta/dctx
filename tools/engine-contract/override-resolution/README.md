<!-- SPDX-License-Identifier: Apache-2.0 -->
# Engine override resolution

Run from the repository root on a POSIX host with Python 3.11+, Go 1.23+,
and Node.js 18+.

```sh
python3 -B tools/engine-contract/override-resolution/test_resolution.py
```

The check runs the public clients from a caller directory and gives each client
a separate project directory. Both directories contain an executable at the
same relative path. The stubs answer the RPC handshake with the repository
engine version and return distinct `languages` markers. Each stub also checks
that the client starts it in the project directory.

| Binding | Relative explicit option | Relative `DCTX_BIN` | Absolute caller path through either |
| --- | --- | --- | --- |
| Python | Caller via `binary` | Caller | Caller |
| Go | Caller via `Options.Binary` | Caller | Caller |
| Node.js | Project via `binaryPath` | Project | Caller |

Each result is compared byte for byte with its expected marker, including
non-ASCII text and trailing newlines. All six absolute-path results must match
the same caller marker. An absolute executable path is portable across the
three bindings.

The check uses the local package sources and the installed Go toolchain. It
requires no downloads or real engine. It removes its temporary fixtures after
the run and checks diagnostic escaping with a path component containing a
newline, an escape character, and a quote.
