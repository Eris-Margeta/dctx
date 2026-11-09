<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Choosing how to use dctx

Compare dctx with **grep plus reading files**, and choose by the work you need to
do. Smaller tool output is useful, but total agent cost also includes startup
instructions, tool definitions, conversation history and the number of model
turns. A tool call can require another model request carrying that context again;
provider caching can change its billed cost. Count both calls and session tokens.

## Available surfaces and publication status

| Modality | Install | Status | Best for |
|---|---|---|---|
| CLI / interactive terminal | [Prebuilt archives](https://github.com/Eris-Margeta/dctx/releases/tag/v1.3.1), or [build from source](../README.md#from-source) | Available now; crates.io publication pending | One-off chat (`pack --clipboard`), shell agents, scripts and CI; run `dctx` for the terminal menu |
| MCP server | Included in the CLI; configure `dctx mcp /absolute/project` | Available now, stdio | Claude Code, Cursor and Codex tool calls on a scoped project |
| Rust library (`dctx` facade) | Tagged Git dependency in the [library section](#rust-library-embed-the-facade) | Available from Git; crates.io publication pending | Embedding pack, map, templates and cache in Rust tools and editors |
| Desktop app | [Build the Tauri app](../apps/dctx-desktop/README.md#quick-start) | Tauri 0.11 preview today; GPUI rebuild planned | Selecting projects and generating snapshots in a GUI |
| Python, Node and Go wrappers | `npm install dctx` for Node; CI artifacts for Python and Go; see [language wrappers](#python-node-and-go-wrappers) | Node published on npm; all wrappers tested in CI | Calling the engine over `dctx rpc` from Python, Node or Go projects |

The npm release publishes `dctx` with pinned `@tejl/dctx-<platform>` optional
dependencies. Python and Go wrappers are available as CI artifacts; the Rust
facade is available as a tagged Git dependency. Native release archives remain
available for system installation or an explicit `DCTX_BIN` path.

## Choose by task

| Your situation | Start here | Avoid it when |
|---|---|---|
| Human, one-off chat | `dctx pack . --clipboard` | The selected tree is too large to paste or contains irrelevant material |
| Short agent task | Grep, then read the matching file or lines | Add dctx only if a focused query saves enough work to cover setup and extra turns |
| Long agent session / big files | `find` → `read --json`, with `skeleton` for orientation | The needed file is already small or a grep plus narrow read gives the same answer |
| Huge tree, monorepo, data or Markdown corpus | Run from a relevant subfolder; apply ignore rules before scanning | You need an unscoped index of 100 GB+ or expect a budget flag to reduce the scan |
| MCP client | `dctx mcp .` in a scoped root | Tool definitions and extra model turns outweigh focused-read savings |
| CI / repeated prompt prefix | Deterministic `pack` or scoped map output saved as an artifact | You expect dctx alone to guarantee provider cache hits |

## Human, one-off: pack to clipboard

From the project or relevant subfolder:

```sh
dctx pack . --clipboard
```

Paste the resulting structured context into your chat. The local clipboard must
be available. To inspect or save the same selection instead:

```sh
dctx pack . --output context.txt
```

Do not use an unbounded whole-project pack for a large tree or when only one
function matters. Narrow the root first, or use map reads. Pack's `--ext rs,toml`
and `--small 1` filters can restrict extensions and file sizes (MB); they are
pack filters, not map index limits. Inspect the content before sharing it.

## Short agent task: grep can be enough

If you know a name and expect only a few file reads, use the existing shell:

```sh
grep -R -n -F --include='*.rs' 'struct Session' crates/dctx/src
sed -n '130,170p' crates/dctx/src/session/mod.rs
```

Choose the line range from the search result; reading a narrow range is an
honest baseline alongside whole-file reads. If a symbol read is useful, call
`dctx map find Session` through the existing shell. Loading an MCP server
adds tool descriptions to starting context; installing verbose agent
instructions can add more. Do not add that setup for a tiny edit unless you
measure a benefit.

```sh
cd crates/dctx-lex-go
dctx map find Add --budget 500 --limit 5
dctx map read 's:tests/fixtures/basic.go#Add' --json
dctx map skeleton tests/fixtures/basic.go --budget 1000
```

For your project, use the exact ID returned by `find`. Both text `read` and
`read --json` include verified source bytes; JSON carries them in `source_utf8`. Skeletons
show declaration signatures without bodies. If an answer has `more` and `next`,
pass that opaque `next` value with `--cursor` on the same request, and count the
extra call. Budgets are estimated output tokens, not a model tokenizer promise.

## Huge trees: scope before indexing

A 100 GB+ folder, including a 200 GB working tree, is a container of possible
scopes, not a promise that dctx can index it all at once. Source volume, visited
entries, record count and cache size are separate limits. Markdown headings
produce document nodes, so mostly text trees can still have large indexes.

Map's shared cache stores a lifecycle ledger (`ledger.v1`), policy (`policy.v1`),
serialized generations and their view manifests in managed object files. A map
generation contains file inventory, digests and stat stamps, declarations,
summaries, signatures, source spans and edges. It is not a copy of the whole
source tree or an embedding store; exact source reads still verify live files.
A compatible unchanged generation can be reused across CLI processes.

The product hard cap is **2 GiB (2,147,483,648 accounted bytes)** across the shared
cache. Defaults also include a 1.5 GiB soft watermark and a roughly 1.2 GiB
cleanup target. Admission evicts reclaimable generations first, preserving live
reader leases. If the new generation still cannot fit, the query fails with a
cache-limit error; it does not quietly return a complete partial index. Repeating
the same unscoped request will not fix this. The cap is not a RAM-usage or exact
filesystem-size guarantee. `cache config --max-bytes` can only tighten it.

Start at the relevant package, not the huge parent:

```sh
cd packages/service-a
dctx map find Config --budget 500 --limit 5
# Equivalent root selection from elsewhere:
dctx map find Config --place packages/service-a --budget 500 --limit 5
```

`--place` selects the scan root. `--in src` narrows find/grep matches **after**
indexing; `--budget`, `--limit` and `--cursor` bound answers, not the tree scan.
Running from a subfolder changes relative paths and IDs; retain the needed ignore
rules there instead of assuming parent project rules were inherited.

Use existing `.gitignore` and a `.dctxignore` at the chosen root to exclude data,
generated output and irrelevant documents before reads, parsing and caching.
For example, merge these project-specific entries into `.dctxignore`:

```gitignore
/data/
/generated/
/archive/
```

The host loads the global ignore file, root and nested `.gitignore` rules, then
the root `.dctxignore`. Built-in sensitive-file and `.git` exclusions still apply.
The global ignore file is `<config dir>/dctx/ignore`: `$XDG_CONFIG_HOME/dctx/ignore`
or `~/.config/dctx/ignore` on Linux and macOS, `%APPDATA%\dctx\ignore` on Windows.
dctx does not read Git metadata, so `.git/info/exclude` and Git's
`core.excludesFile` are not applied; put those patterns in `.dctxignore` or the
global ignore file.
Do not set `DCTX_INDEX_IGNORED=1` when relying on these ignore rules.

The built binary documents these **environment variables**, not scan-limit flags:

| Variable | Default | Accepted range / effect |
|---|---|---|
| `DCTX_MAX_SOURCE_BYTES` | 1,073,741,824 (1 GiB) | 1–17,179,869,184 total source bytes per scan |
| `DCTX_MAX_RECORDS` | 1,000,000 | 1–100,000,000 files, symbols and edges |
| `DCTX_MAX_ENTRIES` | 100,000 | 1–100,000,000 visited files and directories |

For a deliberately smaller scope and fail-fast ceilings:

```sh
DCTX_MAX_SOURCE_BYTES=268435456 DCTX_MAX_RECORDS=250000 DCTX_MAX_ENTRIES=25000 \
  dctx map find Config --place packages/service-a --budget 500 --limit 5
dctx cache status --json
dctx cache gc --dry-run
```

Limits refuse an oversized scan; they do not produce an exhaustive answer from
a truncated scan. Raising source or record ceilings does not raise the 2 GiB
cache cap. A different absolute `DCTX_CACHE_DIR` selects a separate cache
location, not a larger cap. Do not use dctx on the whole huge tree if narrowing
would exclude facts your task requires; use grep and direct reads across the
needed directories, or query several explicit scopes and account for extra calls.

## MCP server: convenient tools with startup context

Configure your MCP client to launch this process in the selected project root:

```sh
dctx mcp .
```

### Client configuration

Install the CLI archive first. Replace both absolute paths below with your
executable and the project to index (on Windows, use `dctx.exe` and JSON-escaped
paths). An explicit project path avoids relying on the client's working directory.
For a source build, point `command` at `target/release/dctx-cli`.

[Claude Code](https://code.claude.com/docs/en/mcp) (`.mcp.json` at the project root)
and [Cursor](https://cursor.com/docs/mcp) (`.cursor/mcp.json`) use this JSON:

```json
{
  "mcpServers": {
    "dctx": {
      "type": "stdio",
      "command": "/absolute/path/to/dctx",
      "args": ["mcp", "/absolute/path/to/project"]
    }
  }
}
```

[Codex](https://developers.openai.com/codex/mcp/) uses `~/.codex/config.toml`
(or `.codex/config.toml` in a trusted project):

```toml
[mcp_servers.dctx]
command = "/absolute/path/to/dctx"
args = ["mcp", "/absolute/path/to/project"]
```


### Tools and startup cost

It exposes ten tools: `map`, `find`, `neighbors`, `explain`, `read`, `skeleton`,
`grep`, `hubs`, `path`, `export`. These are query tools, not a pack or cache-admin
interface. Export emits the whole graph without the normal answer-token cap.
Both text and JSON `read` answers include verified source bytes. Set the MCP
`read` argument `"json": true` for structured output with `source_utf8`, equivalent
to the CLI's `--json`.

| Payload | Exact UTF-8 bytes | Estimated tokens |
|---|---|---|
| Complete `tools/list` response, request ID `2`, without trailing LF | 6,135 | 1,753 |
| Same response including one LF | 6,136 | — |
| Compact `result.tools` array alone (definitions and schemas) | 6,091 | 1,741 |

The [captured response](assets/mcp-tools-list-1.2.0.json) includes its trailing LF.
Token estimates use `ceil(2 × bytes / 7)`, the CLI's byte heuristic, **not an
actual model tokenizer**. Clients may transform schemas, add wrappers or defer
loading tools, so this is the definition payload's estimated startup cost, not
the client's exact input-token count.

Reproduce from the project root (with `dctx` on PATH):

```sh
python3 - <<'PYCODE'
import json
import subprocess

requests = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "dctx-modality-measurement", "version": "1"}}},
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
]
wire = "".join(json.dumps(r, separators=(",", ":")) + "\n" for r in requests)
reply = subprocess.run(["dctx", "mcp", "."], input=wire.encode(),
                       stdout=subprocess.PIPE, check=True).stdout
line = next(x for x in reply.splitlines() if json.loads(x).get("id") == 2)
definitions = json.dumps(json.loads(line)["result"]["tools"],
                         ensure_ascii=False, separators=(",", ":")).encode()
for name, payload in [("response without LF", line), ("tools array", definitions)]:
    print(name, len(payload), "bytes;", (2 * len(payload) + 6) // 7, "est. tokens")
PYCODE
```

Do not enable MCP solely to save output tokens on a short task. Prefer existing
shell access when it avoids adding definitions; use MCP when the client's tool
workflow makes repeated focused queries useful. Compare total session usage
under the same client and model, including definition loading and extra turns.

## CI / prompt-cache prefix stability

Save deterministic output from a fixed selection as a build artifact:

```sh
dctx pack . --output context.txt
dctx map find Session --json > symbol-context.json
```

Keep generated artifacts outside the next scan or ignore them. Keep the default
`--tree-stamps=off` for stable prefixes. For map,
leave `--meta` off to exclude changing snapshot, generation and age metadata.
Keep stable instructions and context early in the assembled prompt, followed by
changing task text. Use the same template, root, request and binary version.

The [stability report](research/stability-20261105/REPORT.md) measured 33/33 text
and 33/33 JSON answers unchanged after unrelated edits on three Linux corpora,
and identical output over five runs. It measured answer bytes, not provider
cache hits, billed tokens or whole-session latency. A bare `touch` leaves default
pack output unchanged; a real edit can preserve the bytes before the edited
file. Do not assume every edit preserves the entire prompt, or that provider
cache retention and minimum-prefix requirements are controlled by dctx.


## Rust library: embed the facade

The supported entry point for Rust integrations is the [`dctx` facade](../crates/dctx),
which exposes `Session`, `SnapshotRequest`, `MapRequest`, `PackRequest`, sinks,
templates and cache operations. Internal engine crates are implementation details.
The facade is available from Git now; **crates.io publication is pending**. In your
application's `Cargo.toml`:

```toml
[dependencies]
dctx = { git = "https://github.com/Eris-Margeta/dctx", tag = "v1.3.1" }
```

The [ten-line pack example](../README.md#use-as-a-library) is compiled and run as a
[workspace doctest](../examples/library/README.md). It uses an in-memory source and
an explicit 8 MiB ephemeral storage ceiling, so it needs no CLI subprocess or
project scan. Run it from this repository's root:

```sh
cargo test --locked -p dctx-library-example --doc
```

The host supplies source, clock, cancellation, selection policy, grants, storage
and place/view names. The facade does not discover a home directory, global
configuration, Git executable or plugin host; it does not print to stdout.
The example's `println!` belongs to the caller. A `VecSink` collects the encoded
pack or query answer for your application to consume.

For filesystem use, open an `FsSource` at a scoped root and supply a live `Clock`.
The source does **not** automatically load `.gitignore`; the host must pass its
ignore-rule texts through `RuleTexts` / `SelectionPolicy`. Choose storage ceilings
and persistence explicitly. The CLI supplies its own config, rules and cache
lifecycle, so copying a library call without that host policy is not equivalent
to invoking the CLI.

## Desktop app: Tauri preview today

The [desktop app](../apps/dctx-desktop/README.md) is a Tauri 2 app, currently the
0.11 preview. It uses `dctx-desktop-api` and the shared Rust facade. Build it from
source using the app's [quick start](../apps/dctx-desktop/README.md#quick-start);
the CLI archives do not install a desktop application. A GPUI rebuild is planned,
not the current downloadable or buildable desktop implementation.

## Python, Node and Go wrappers

Thin wrappers over the versioned [`dctx rpc` protocol](protocol/README.md) live in
[packages/python](../packages/python/README.md),
[packaging/npm/dctx](../packaging/npm/dctx/README.md) (which also forwards the
`dctx` CLI command) and [packages/go](../packages/go/README.md). Each starts the
native engine, passes CLI argument strings and returns the engine's bytes
unchanged; CI checks byte parity with the CLI. Install the Node wrapper with
`npm install dctx`; npm includes the matching pinned `@tejl/dctx-<platform>`
optional dependency. Install Python and Go wrappers from CI artifacts or a
checkout, as each README describes, and point them at an engine
from the [GitHub release archives](https://github.com/Eris-Margeta/dctx/releases)
that has `dctx rpc`. Their arguments carry full CLI authority, so never pass
untrusted values as `--place`, `--output` or dash-leading arguments.

Without a wrapper, any language can invoke the CLI as a subprocess and consume
`--json` map answers. Use the Rust facade when you need an in-process engine.
