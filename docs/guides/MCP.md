<!-- SPDX-License-Identifier: Apache-2.0 -->
# MCP server

`dctx mcp [PATH]` serves JSON-RPC 2.0 over stdio, one message per line. The
client starts the process; tools run in `PATH`, or the current directory when
omitted. Diagnostics go to stderr. `dctx serve` and `dctx serve --mcp` accept
the same options. No separate server installation is needed.

## Choose the tool surface

| Mode | Command | Tools and descriptions |
| --- | --- | --- |
| Default | `dctx mcp [PATH]` | All 10 tools, original descriptions. Also `--profile default`. |
| Lean | `dctx mcp --profile lean [PATH]` | `find`, `read`; shorter descriptions. |
| Custom | `dctx mcp --tools find,read,grep [PATH]` | Only the named tools; shorter descriptions. |

The available names are `map`, `find`, `neighbors`, `explain`, `read`, `hubs`,
`grep`, `skeleton`, `path`, `export`. Each maps to `dctx map <name>` and runs
the same implementation. See [Map mode](MAP-MODE.md) for query behavior,
budgets, pagination, indexing and cache limits.

Lean suits sessions that locate symbols or documentation and then read their
source by ID. Include `grep` when exhaustive literal search within indexed
files is needed. Use the default surface for graph navigation, outlines and
exports. All modes retain every parameter, type, required field and property
description of each selected tool, including output budgets and continuation
cursors where supported. Only tool availability and top-level descriptions
change. A subset does not reduce indexing scope or raise cache limits.

Choose either `--profile` or `--tools`, once. Both `--profile=lean` and
`--tools=find,read` also work. Options may precede or follow `PATH`; use `--`
before a path starting with `-`. Names are case-sensitive, comma-separated,
without spaces. Unknown names/profiles, empty entries, duplicates, missing
values and conflicting/repeated selectors are usage errors (exit 2), before
the protocol starts. An inaccessible project path is exit 3.

The tool list follows the default order regardless of the order in `--tools`.
Unselected tools are absent from `tools/list`; calling one returns JSON-RPC
error `-32602` without executing it. Restart the server to change the selection.

## Measured overhead

Measurements below use a release build of dctx 1.2.0 (`fd82034`) on Linux
x86-64, Python 3.13.5, on 2026-11-07. They come from the running binary's
`tools/list`, not a checked-in schema approximation. Reproduce them from the
repository root with the [stdlib measurement script](../../tools/mcp-measure/measure.py):

```sh
RUSTC_WRAPPER= cargo build -p dctx-cli --release --locked --target-dir target
python3 tools/mcp-measure/measure.py target/release/dctx-cli
```

Cargo names the development executable `dctx-cli` (`dctx-cli.exe` on Windows);
releases install it as `dctx`. The script also accepts an installed binary's
absolute path. See its [measurement notes](../../tools/mcp-measure/README.md).

| Profile | Tools | Tool-definition bytes | Estimated tokens | Input-schema bytes | tools/list wire bytes | Initialize + list wire bytes | Estimated startup tokens |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Full (`default`) | 10 | 6,091 | 1,522.75 | 4,894 | 6,136 | 6,278 | 1,569.50 |
| Lean | 2 | 1,450 | 362.50 | 1,251 | 1,495 | 1,637 | 409.25 |

Tool-definition bytes count the compact JSON `result.tools` array. Input-schema
bytes sum only its `inputSchema` objects, already included in that array; do
not add them again. Wire columns count the actual UTF-8 reply lines, including
JSON-RPC envelopes and newline framing. Startup uses request IDs `1` and `2`
and protocol `2025-06-18`. Estimates here are **bytes / 4**, not measured model
tokens or billing. Clients may transform, defer loading, cache or repeatedly
include tool schemas, and add their own instructions and wrappers.

MCP is the training wheels: it works, it is fully supported, and it costs you
about 1,570 tokens of start-up per session with the full profile (about 409 with
lean). The binary is the bike.

Those are startup estimates under the convention above. The binary is the
single source of truth: MCP transports engine output and advertises its tools;
it does not produce a separate answer. Lean reduces schema bytes by limiting
the available tools, while preserving their full parameter schemas.

### Result wrapping

The fixture's `read` call is the same engine operation as
`dctx map read m:read.rs --budget 2000`. The script asserts that decoded
`content[].text` equals CLI stdout **byte-for-byte**, for both text and
`json: true`. There is no `pack` tool on this MCP surface.

| Read output | CLI stdout bytes | MCP result bytes | MCP wire bytes | Wire / CLI |
| --- | ---: | ---: | ---: | ---: |
| Text | 106 | 160 | 196 | 1.849× |
| Facade JSON (`json: true`) | 500 | 637 | 673 | 1.346× |

`MCP result` counts the compact `result` object; `MCP wire` also counts the
JSON-RPC envelope and newline. These calls use two-digit request IDs. This is
a deliberately small source with quotes, backslashes, Unicode and newlines.
The fixed envelope is conspicuous on short results; larger results amortize
it. Escaping depends on content, so these ratios are examples, not a universal
multiplier. Facade JSON is returned inside a text block, so its quotes and
backslashes are escaped again. Wire inflation does not imply the same increase
in model context: a client may unwrap the text before presenting it.

### Process lifetime, cold calls and concurrency

The client keeps a stdio server subprocess for its MCP connection, normally
for the session, rather than spawning `dctx` for every tool call. The server
exits when stdin closes. The client controls restart and connection sharing
across chats. Tool selection and project path stay fixed until restart.
Each call still opens the shared engine's map session and checks the project;
keeping the stdio process alive does not pin an in-memory index for all calls.

The script creates 26 Rust files (24,629 bytes) with an isolated empty dctx cache, measures
one first `find`, then seven identical warm calls in the same process. Cold
means **empty dctx cache**, not cold OS page cache: fixture creation already
touched its source bytes. Python, pipes, scanning and engine/cache work are
included; model/network time and editor startup are excluded. These wall times
are observations on this machine, not latency guarantees.

<!-- mcp-measure-timings:start -->
| Wall-time measurement (lean) | Milliseconds |
| --- | ---: |
| Spawn + initialize + tools/list | 10.369 |
| First find, empty dctx cache | 42.863 |
| Spawn through first find | 53.232 |
| Warm find median (7 calls) | 17.388 |
| Pipelined 8 finds: first reply | 46.690 |
| Ping queued after burst | 491.812 |

Burst reply completion offsets (ms): 46.690, 236.693, 403.363, 433.573,
443.885, 459.316, 476.297, 491.572. Replies followed request order.
<!-- mcp-measure-timings:end -->

This was a shared sandbox: warm samples ranged from 13.814 to 184.804 ms.
The [raw sample](../../tools/mcp-measure/sample.json) includes every warm and
burst timing and a fixture digest. Repeat on your machine before drawing
performance conclusions.

Requests can be outstanding together on the connection, but the current server
executes them **serially in input order**. The script writes eight `find`
requests without waiting for replies, then queues a `ping`. The ping replies
after all eight tools: a slow tool blocks later requests, including ping.
This agrees with the synchronous
[stdio loop](../../crates/dctx-cli/src/mcp/mod.rs). Pipelining does not give
parallel engine execution. Separate processes have separate queues and can
still contend on shared cache or filesystem resources; this measurement does
not establish multi-process throughput or cancellation behavior.

## Errors and bounded results

A known tool with bad arguments, or an engine execution failure, returns a
JSON-RPC success envelope containing `result.isError: true` and a text block
with the diagnostic. For example, calling `read` without `id` gives:

```json
{
  "content": [{"type": "text", "text": "id is required and must be a non-empty string"}],
  "isError": true
}
```

Successful calls omit `isError`. The script verifies the error above and a
subsequent ping on the same process. The tool text carries the failure message;
there is no separate CLI exit-code or remediation-hint field. A
missing/unselected tool name is instead a protocol error (`-32602`); an unknown
method is `-32601` and malformed JSON
is `-32700`. A missing node can be a normal engine `gone` outcome, so inspect
the returned page as well as `isError`. Errors before protocol startup use
the exit codes described above and stderr; they cannot return a tool result.

Clients and model providers can independently truncate or replace a result.
There is no universal MCP client output limit, and dctx cannot detect every
downstream cut. A page that fits the engine budget can exceed a client's cap
after escaping and wrappers. A truncated response may lose its `next` cursor.

- Start with lean `find` using a small `limit`, then `read` the returned ID.
  Set an explicit `budget` when a client's result cap requires smaller pages;
  leave space for the MCP and client envelopes. Engine budget estimates use
  `ceil(2 * UTF-8 bytes / 7)`, separately from the bytes / 4 startup estimate.
- Use `json: true` when you need machine-readable page metadata. Parse the
  JSON inside the MCP text block, inspect `more` and `next`, then pass an
  opaque continuation value back as `cursor` on the same tool and query.
  Text pages also print continuation metadata. Follow an actual cursor;
  `next: "map"` is a suggested action for an identity outcome, not a cursor.
- `find` and budget-cut `read` pagination are exercised by the script. If a
  cursor becomes stale after source/index changes, restart the original query.
  A too-small budget can refuse the call rather than split an indivisible
  record; narrow the query or raise the budget within the client limit.
- Source reads also have a line cap: normally 120, up to 400 with `full`.
  `[DCTX_TRUNCATED]` marks a budget cut with a continuation;
  `[DCTX_OMITTED]` marks source beyond the selected line cap and does not
  guarantee a cursor. Use explicit absolute `lines` windows within the node
  for later source. The script verifies `lines: "35-37"` against fixture bytes.
  Start a window without an old `cursor`; both select the starting point.
- If the client drops a tail, retry the original query with a smaller page
  budget/limit, or use the CLI to capture output directly. Choose a custom
  surface when graph tools are needed; whole-graph `export` has no page cursor.

These controls preserve engine receipts and continuation metadata; they do
not justify treating a client-truncated fragment as a complete answer.
See [Map mode](MAP-MODE.md) for indexing, freshness and cache policy.

## Client configurations (lean)

Replace `/absolute/path/to/dctx` and `/absolute/project/path` with real absolute
paths, preserving each array element as one argument. Absolute executables
avoid GUI-versus-shell `PATH` differences. Merge entries into existing config;
restart/reconnect the MCP connection after changes.

**Verification, 2026-11-07:** JSON/TOML syntax was parsed locally; the common
dctx launch arguments, initialize, tools/list, find and read were exercised
against the release binary by the script. Configuration keys were checked
against the linked official documentation. Local client checks used isolated
configuration directories:

| Client | What was verified |
| --- | --- |
| Claude Code 2.1.316 | The registration command below wrote the expected project `.mcp.json`. |
| Codex CLI 0.166.0 | `mcp get --json` accepted the command and args through equivalent TOML overrides. |
| OpenCode 2.2.0 | `debug config` read the v2 entry; server logs confirmed MCP connection with two tools. ACP initialization and `session/new` with a forwarded dctx server also succeeded, with two tools discovered. |
| Cursor, Zed, JetBrains AI Assistant | Documentation and JSON syntax checked; editors were unavailable. |

Authentication, model invocation and editor UIs were **not run**. Confirm that
the MCP connection exposes only `find` and `read` before relying on a particular
integration. Registration/config parsing alone does not prove a client session.

### Claude Code

Run in the project, using the documented
[stdio registration syntax](https://code.claude.com/docs/en/mcp):

```sh
claude mcp add --transport stdio --scope project dctx -- /absolute/path/to/dctx mcp --profile lean /absolute/project/path
```

This writes the project `.mcp.json`. Inspect the connection with `claude mcp
list` or `/mcp` in Claude Code.

### Codex

Merge into `~/.codex/config.toml`, using the official
[MCP configuration](https://developers.openai.com/codex/mcp):

```toml
[mcp_servers.dctx]
command = "/absolute/path/to/dctx"
args = ["mcp", "--profile", "lean", "/absolute/project/path"]
```

Inspect with `codex mcp list` and `/mcp` in an interactive session.

### Cursor

Merge into the project's `.cursor/mcp.json` (or `~/.cursor/mcp.json` globally),
following [Cursor's stdio configuration](https://cursor.com/docs/context/mcp):

```json
{
  "mcpServers": {
    "dctx": {
      "type": "stdio",
      "command": "/absolute/path/to/dctx",
      "args": ["mcp", "--profile", "lean", "/absolute/project/path"]
    }
  }
}
```

Review the enabled server and tools in Cursor's MCP settings.

### OpenCode

For **OpenCode v2**, merge into the project's `opencode.json` (JSONC is also
supported). Current [MCP documentation](https://opencode.ai/docs/mcp-servers/)
places servers under `mcp.servers`; v1 configurations use a different shape.

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "dctx": {
        "type": "local",
        "command": ["/absolute/path/to/dctx", "mcp", "--profile", "lean", "/absolute/project/path"]
      }
    }
  }
}
```

Servers connect automatically; inspect with `opencode mcp list`. Check your
installed major version's documentation before importing this snippet.

## ACP: editors connect to agents, agents connect to dctx

[ACP (Agent Client Protocol)](https://agentclientprotocol.com/protocol/session-setup)
connects editors to coding agents. **dctx is not an agent and ships no ACP
server.** Do not put `dctx mcp` in an editor's `agent_servers` entry. Use an
ACP-capable agent, such as OpenCode, and mount dctx as an MCP tool server:

```text
Zed / JetBrains AI Assistant --ACP--> agent --MCP stdio--> dctx binary
```

The editor can forward configured MCP servers during ACP session setup; an
agent can also read its native MCP configuration. Agent support and editor
versions determine forwarding. Configure dctx in one place to avoid duplicate
connections and tool definitions. The verification limits stated above apply
to both examples below. A headless probe completed OpenCode 2.2.0's ACP v1
`initialize` and `session/new` with dctx in `mcpServers`; its log confirmed two
MCP tools connected. This verifies the agent side of forwarding without a
model call. Editor settings were documentation-checked; **no actual
editor-to-agent session was tested**.

### Zed with OpenCode

Install and authenticate OpenCode separately. Merge into Zed's settings
(open with `zed: open settings file`), following its official
[custom MCP settings](https://zed.dev/docs/ai/mcp) and OpenCode's
[ACP setup](https://opencode.ai/docs/acp/):

```json
{
  "agent_servers": {
    "OpenCode": {
      "type": "custom",
      "command": "/absolute/path/to/opencode",
      "args": ["acp"]
    }
  },
  "context_servers": {
    "dctx": {
      "command": "/absolute/path/to/dctx",
      "args": ["mcp", "--profile", "lean", "/absolute/project/path"],
      "env": {}
    }
  }
}
```

Start an OpenCode external-agent thread. Confirm `find` and `read` are visible;
use `dev: open acp logs` to inspect forwarding if they are absent. If the
installed agent does not accept forwarded MCP servers, use its native MCP
configuration (the version-appropriate OpenCode entry above) instead. Zed
documents this boundary in [External Agents](https://zed.dev/docs/ai/external-agents).

### JetBrains AI Assistant with OpenCode

In AI Assistant, choose **Add Custom Agent** and merge into
`~/.jetbrains/acp.json`, following the official
[ACP settings](https://www.jetbrains.com/help/ai-assistant/acp.html):

```json
{
  "default_mcp_settings": {"use_custom_mcp": true},
  "agent_servers": {
    "OpenCode": {
      "command": "/absolute/path/to/opencode",
      "args": ["acp"],
      "env": {}
    }
  }
}
```

Then go to **Settings → Tools → AI Assistant → Model Context Protocol (MCP)**,
add a stdio server with this
[MCP configuration](https://www.jetbrains.com/help/ai-assistant/mcp.html), and apply:

```json
{
  "mcpServers": {
    "dctx": {
      "command": "/absolute/path/to/dctx",
      "args": ["mcp", "--profile", "lean", "/absolute/project/path"]
    }
  }
}
```

Enable **Pass custom MCP servers** for the agent in AI Assistant's Agents
settings, select OpenCode in AI Chat, and verify its tool list. If forwarding
is unsupported by the installed combination, use OpenCode's native MCP config.
This recipe was checked against AI Assistant 2026.2 documentation; earlier
versions can differ, and that documentation excludes ACP agents in WSL.

## Measure whole sessions too

Compare whole sessions against **grep plus reading files**: count calls, include
startup instructions and tool definitions in each model request, and record
input/output tokens, cache behavior and task correctness. Reduced tool output
alone does not establish a session saving. Bounded `find` then `read` is useful
when repeated reads can select a small relevant node from a large file; choose
the surface around the session's tasks.
