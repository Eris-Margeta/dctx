<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# CLI exit codes

`dctx map`, `dctx pack`, `dctx mcp`, `dctx languages`, an unknown command and a non-UTF-8 argument follow one contract:

`dctx serve [PATH]` and `dctx serve --mcp [PATH]` are aliases for `dctx mcp [PATH]`.
They use the current directory when PATH is omitted, including a directory named
`fixtures`; tools index paths relative to that directory. `dctx read` is an alias
for `dctx map read`, with the same flags and exit codes.

| Code | Meaning | Examples |
|---|---|---|
| 0 | ok: the command answered | a `find` page, including one with no hits; `--help` |
| 1 | no result: the command ran, but the named target or input has no answer | `map read m:nope.py` (an id not in the index); `map read logo.png` (a binary file); `pack` of a directory with no packable file |
| 2 | usage: the arguments are wrong | an unknown command or flag; a missing target; `--depth 9`; an empty `find` query; a stale `--cursor`; `m:../outside`; a non-UTF-8 argument |
| 3 | index error: the place, source, cache or index could not be opened, read, built or written | `--place /no/such/dir`; a corrupt or busy cache; a source over `DCTX_MAX_SOURCE_BYTES`; an `--output` file that cannot be written |

Every failure prints two lines on stderr: `❌ Error: <what went wrong>` (prefixed with the command, as `map: …`), then `hint: <what to do next>`. No input should reach a Rust panic; `crates/dctx-cli/tests/cli_errors.rs` and `<repo>/scripts/checks/V1-5.zsh` run twelve bad inputs and assert the code, the message, the hint and the absence of `panicked`.

## Details

- An id the index does not hold is a facade outcome (`gone`), not an error. `dctx map` prints the `gone` page on stdout, then exits 1 with the hint. `dctx mcp` returns the same page to the client as a normal tool result.
- An empty `find` or `grep` page is an answer and exits 0. Exit 1 is for a named target that is not there.
- A binary target is detected before the scan: a NUL byte in the first 8 KiB of the file named by `map`, `skeleton`, `explain` or `read` (path or `m:` id). `neighbors` and `path` still accept such a module, since source files can import it.
- `dctx pack` exits 1 only when no view filter is set (`--ext`, `--include`, `--exclude`, `--binary`, `--empty`, `--non-empty`, `--large`, `--small`, `--hidden`, `--debug-ignored`). A filter that matches nothing still prints an empty page and exits 0. The page and the `pack bytes=…` line are printed before the exit-1 message. `pack` exit 3 covers source, scan, render and output failures.
- A reader that closes stdout early (`dctx pack . | head -1`) ends the run with exit 0 and no message, as `cat` does; every other failure keeps its code.
- `dctx pack --for <TEXT>` exits 1 when no name in TEXT matched the index (the page is printed first), 2 for a bad flag or a `--budget` outside 100..100000 (or `--budget` without `--for`), and 3 when PATH is not a directory or the index fails.
- `dctx mcp [--profile default|lean | --tools NAME,...] [PATH]` exits 3 when PATH cannot be entered and 2 for invalid arguments (including unknown tool names/profiles). See the [MCP guide](guides/MCP.md). Once serving, a failed tool call goes back to the client as a result with `isError: true`; unknown or unselected tools return JSON-RPC `-32602`. The server exits 0 at end of input.
- A facade refusal of the query (`InvalidRequest`, `BudgetTooSmall`, `StaleContinuation`, `Stale`, `UnsupportedCapability`, `UnsupportedVersion`) is 2; `PolicyDenied` and `PolicyUnavailable` are 1; every other facade error, and every error of the snapshot build, is 3. The mapping is `dctx_cli::exit::from_facade`.
- `map hubs`, `map path` and `map export` report their facade errors as text: an `unknown id` end of `map path` is 1, anything else 3.

## Outside the contract

These commands keep their earlier behaviour, exit 1 for every failure, with no hint line:

- `dctx cache …` and `dctx template …`;
- the legacy snapshot `dctx PATH` once its arguments parse (a bad snapshot flag is exit 2);
- `dctx plugin …` and `dctx git …`, which return the plugin host's own codes.

## Engine RPC

`dctx rpc` uses the same operation paths and numeric taxonomy over
[versioned NDJSON](protocol/README.md). Each reply carries `engine_version`
and echoes its request id. An error contains the CLI code, diagnostic, hint and
any stdout emitted before failure (including gone pages). Protocol/framing
refusals use code 2 and leave the session usable. Cache status retains its
legacy code-1 failures. An error reply does not change the server's process exit
status: normal EOF exits 0, transport I/O errors exit 3, and broken stdout exits
quietly with 0. The server performs no piggyback auto-clean.

`pack --for TEXT --json` now emits the facade's context JSON page; its text
member contains the same budgeted context page as plain `pack --for TEXT`.
