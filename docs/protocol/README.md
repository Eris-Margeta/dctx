<!-- SPDX-License-Identifier: Apache-2.0 -->
# dctx engine RPC v1

`dctx rpc` is the engine protocol for pip, npm and future Go, Java and .NET
packages. The binary is the single source of truth. Read
[PHILOSOPHY.md](../PHILOSOPHY.md) before implementing a client.

Start the engine in the project directory. stdin and stdout carry UTF-8,
newline-delimited JSON, one request and one response per line. Diagnostics go to
stderr. RPC does not piggyback CLI auto-clean. No shell, daemon, network listener
or language runtime is needed.

Send `hello` first:

```json
{"id":"init","verb":"hello"}
{"id":"init","engine_version":"1.2.0","result":{"protocol_version":"1.0.0","verbs":["hello","pack","context","map","find","read","skeleton","explain","neighbors","grep","hubs","path","languages","cache_status"]}}
{"id":1,"verb":"find","args":["ping","--limit","1"]}
```

`protocol_version` has its own semver; it is independent of `engine_version`.
Clients inspect the hello reply before issuing operations and reject unsupported
major versions. A repeated hello is allowed and has no side effects. Hello takes
no arguments. Every request requires a string or signed 64-bit integer `id`.
Responses echo the id and always include `engine_version`, including errors.
Invalid or absent IDs produce `id: null`. IDs need not be unique; clients should
use unique IDs when correlating requests.

`args` is an optional array of CLI argument strings, defaulting to `[]`. Each
string is one argument; there is no shell splitting or interpolation. RPC
selects `--json` by default; an explicit `--json` is also accepted. All other
arguments, including budgets, cursors, paths, selection flags and output
destinations, go to the existing command path. CLI refusals remain refusals;
RPC never retries a different request. Relative paths resolve against the
engine's working directory. Use `--place PATH` for map operations or a positional
PATH for pack operations.

| RPC verb | Corresponding CLI operation |
| --- | --- |
| `pack` | `dctx pack [PATH] --json`, including `--for TEXT` |
| `context` | `dctx context TARGET --json`; legacy `--for TEXT` selects task context |
| `map` | `dctx map map TARGET --json` |
| `find` | `dctx map find QUERY --json` |
| `read` | `dctx map read TARGET --json` |
| `skeleton` | `dctx map skeleton TARGET --json` |
| `explain` | `dctx map explain ID --json` |
| `neighbors` | `dctx map neighbors ID --json` |
| `grep` | `dctx map grep LITERAL --json` |
| `hubs` | `dctx map hubs --json` |
| `path` | `dctx map path FROM TO --json` |
| `languages` | `dctx languages --json` |
| `cache_status` | `dctx cache status --json` |

A successful operation returns `result: {"output":"..."}`. After decoding the
outer JSON string, `output` is **exactly the CLI stdout bytes**, including all
trailing newlines. It is deliberately opaque. A pack summary includes the CLI's
`pack bytes=...` report line after its JSON page; clients must not assume the
string is one JSON document. `pack --for TEXT --json` returns the engine's JSON
v2 context page with `verb: "context"`, `text` (the whole budgeted text page) and
`hits`. Budgeting applies to the text page before JSON escaping. Text context
output continues to use the same engine selection and renderer.

Named context returns ranked content-addressed chunks and supports opt-in delivery
sessions. See [named context and chunk IDs](../CONTEXT.md) for the shared CLI, MCP,
and RPC contract, including final encoded budgets and reference lifetimes.

Errors return `error: {"code":2,"message":"...","hint":"...","output":""}`.
Codes reuse [CLI-EXIT-CODES.md](../CLI-EXIT-CODES.md): 1 means no result, 2 means
usage or protocol refusal, and 3 means index/source/I/O failure. `cache_status`
uses the same codes as `dctx cache status`.
Clients branch on the numeric code, never diagnostic prose. `error.output` holds
any stdout emitted before failure: a gone page or an empty context pack is not
lost. MCP treats a gone page as an ordinary tool result, as documented in the CLI
contract; its page bytes still match `error.output`.

Every line produces one reply. A blank line, malformed JSON, invalid UTF-8,
unknown verb, missing id, invalid arguments or oversized frame returns a usage
error and the session continues. A syntactically invalid frame uses `id: null`;
a valid id in an otherwise invalid request is preserved. Requests are limited to
1,048,576 bytes before LF (including a CR in CRLF). Oversized frames are drained
through their LF without retaining their contents. A final frame at EOF is
processed without requiring a trailing LF. Responses are complete, never
truncated to a transport budget, and flushed individually. An actual stdin or
stdout I/O failure ends the session with exit 3; a closed stdout pipe exits
quietly with code 0.

The session is synchronous: requests execute and replies are emitted in input
order, with at most one operation running. A later request cannot overtake a
slow operation. There is no in-band cancellation in v1: `cancel` is an unknown
verb (usage error). EOF drains all complete submitted frames, including a final
unterminated frame, then exits 0 even when operations returned errors. To abort
running or queued work, terminate the engine process using the host's process
API. No reply is promised for aborted work. Closing stdin alone is graceful
shutdown, not cancellation. The engine owns cache recovery on subsequent runs.

Minor protocol versions only add optional fields. They must not rename, remove,
change the meaning or narrow the accepted values of an existing field. Clients
ignore unknown fields in requests and responses. Major versions may rename or
remove fields and operations. Engine release updates do not imply a protocol
version change; the inner JSON page version is also independent (`v: 2`).

The [committed schema](dctx-rpc-v1.schema.json) is generated from the facade's
Rust wire types in `crates/dctx/src/rpc.rs`, behind the `rpc` feature. It covers
every request verb, hello, successful output and error envelope. The output
string is opaque by contract; its contents retain the CLI's format. Regenerate:

```sh
cargo run -p dctx-cli --example rpc_schema > docs/protocol/dctx-rpc-v1.schema.json
```

The regular workspace test gate byte-compares the committed schema with its
regeneration. It also compares sixteen fixed-corpus operation outputs against
the CLI and every corresponding advertised MCP tool, tests CLI error/status
parity, and tests framing, ordering, handshake, unknown fields, cancellation
semantics and malformed-line recovery. The MCP tool list is discovered at test
time so the lean profile can evolve independently. These checks run in the
existing CI workspace test job on each supported OS/toolchain.
