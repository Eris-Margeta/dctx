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

For clients accepting an `mcpServers` configuration, a lean example is:

```json
{
  "mcpServers": {
    "dctx": {
      "command": "dctx",
      "args": ["mcp", "--profile", "lean", "/absolute/project/path"]
    }
  }
}
```

## Startup cost and whole-session measurement

On dctx 1.2.0, with request IDs `1` and `2` and protocol `2025-06-18`, the
`initialize` plus `tools/list` replies measure:

| Surface | UTF-8 wire bytes, including two newlines | Estimated tokens (bytes / 4) |
| --- | ---: | ---: |
| Default | 6,278 | 1,569.50 |
| Lean | 1,637 | 409.25 |

These are server response measurements, not total agent context or billed
tokens. Client tool wrappers, instructions, history and tokenizer choice affect
the actual cost; clients may inject schemas differently. The lean surface saves
schema bytes, but a smaller tool set can require extra calls for some tasks.

Compare whole sessions against **grep plus reading files**: count calls, include
startup instructions and tool definitions in each model request, and record
input/output tokens, cache behavior and task correctness. Reduced tool output
alone does not establish a session saving. Bounded `find` then `read` is useful
when repeated reads can select a small relevant node from a large file; choose
the surface around the session's tasks.
