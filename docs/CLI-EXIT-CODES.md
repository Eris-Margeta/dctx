<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# CLI exit codes

`dctx map`, `dctx pack`, `dctx context`, `dctx doctor`, `dctx rpc`, `dctx languages`, `dctx cache`, `dctx template`, a bare `dctx` without a terminal, an unknown command and a non-UTF-8 argument follow one contract:
Pack and map use the current directory unless a project root is specified,
including a directory named `fixtures`; paths are relative to that root. `dctx read` is an alias
for `dctx map read`, with the same flags and exit codes.

| Code | Meaning | Examples |
|---|---|---|
| 0 | ok: the command answered | a `find` page, including one with no hits; `--help` |
| 1 | no result: the command ran, but the named target or input has no answer | `map read m:nope.py` (an id not in the index); `map read logo.png` (a binary file); `pack` of a directory with no packable file |
| 2 | usage: the arguments are wrong | an unknown command or flag; a missing target (`dctx context` with no TARGET); `--depth 9`; an empty `find` query; a stale `--cursor`; `m:../outside`; a non-UTF-8 argument; `cache purge` without `--root`/`--all` or `--yes`; a template that does not compile; a bare `dctx` whose stdin or stdout is not a terminal |
| 3 | index error: the place, source, cache or index could not be opened, read, built or written | `--place /no/such/dir`; a corrupt or busy cache; a source over `DCTX_MAX_SOURCE_BYTES`; an `--output` file that cannot be written; `template build /no/such/file` (the message names the path) |

Every failure prints two lines on stderr: `❌ Error: <what went wrong>` (prefixed with the command, as `map: …`), then `hint: <what to do next>`. No input should reach a Rust panic; `crates/dctx-cli/tests/cli_errors.rs` and `<repo>/scripts/checks/V1-5.zsh` run twelve bad inputs and assert the code, the message, the hint and the absence of `panicked`.

## Details

- An id the index does not hold is a facade outcome (`gone`), not an error. `dctx map` prints the `gone` page on stdout, then exits 1 with the hint.
- An empty `find` or `grep` page is an answer and exits 0. Exit 1 is for a named target that is not there.
- A binary target is detected before the scan: a NUL byte in the first 8 KiB of the file named by `map`, `skeleton`, `explain` or `read` (path or `m:` id). `neighbors` and `path` still accept such a module, since source files can import it.
- `dctx pack` exits 1 only when no view filter is set (`--ext`, `--include`, `--exclude`, `--binary`, `--empty`, `--non-empty`, `--large`, `--small`, `--hidden`, `--debug-ignored`). A filter that matches nothing still prints an empty page and exits 0. The page (stdout) and the `pack bytes=…` status line (stderr) are printed before the exit-1 message. `pack` exit 3 covers source, scan, render and output failures.
- A reader that closes stdout early (`dctx pack . | head -1`) ends the run with exit 0 and no message, as `cat` does; every other failure keeps its code.
- `dctx pack --for <TEXT>` exits 1 when no name in TEXT matched the index (the page is printed first), 2 for a bad flag or a `--budget` outside 100..100000 (or `--budget` without `--for`), and 3 when PATH is not a directory or the index fails.
- A facade refusal of the query (`InvalidRequest`, `BudgetTooSmall`, `StaleContinuation`, `Stale`, `UnsupportedCapability`, `UnsupportedVersion`) is 2; `PolicyDenied` and `PolicyUnavailable` are 1; every other facade error, and every error of the snapshot build, is 3. The mapping is `dctx_cli::exit::from_facade`.
- `map hubs`, `map path` and `map export` report their facade errors as text: an `unknown id` end of `map path` is 1, anything else 3.
- `dctx context` with no TARGET is 2; `dctx context --help` prints usage and exits 0. A TARGET that names nothing prints its empty page, then exits 1.
- `dctx cache` exits 2 for a bad flag or value, a purge without `--root PATH`/`--all` or `--yes`, and a `--max-bytes` that would loosen the cap; 1 for a `--root` id the ledger does not hold; 3 when the cache directory, policy or ledger cannot be read or written (the message names the directory).
- `dctx template` exits 2 for a bad flag and for a template that does not compile, and 3 when the template, the `--dir` directory or the `--output` file cannot be read or written (the message names the path). `--output` never writes through a symbolic link and refuses a target that is not a regular file.
- `dctx` with no command opens the interactive menu only when stdin and stdout are terminals; otherwise it prints the short usage as a hint and exits 2.
- `dctx doctor` follows the contract; `dctx rpc` keeps exit 0 at end of input (see Engine RPC below).

## Outside the contract

`dctx plugin …` and `dctx git …` return the plugin host's own codes.

## Engine RPC

`dctx rpc` uses the same operation paths and numeric taxonomy over
[versioned NDJSON](protocol/README.md). Each reply carries `engine_version`
and echoes its request id. An error contains the CLI code, diagnostic, hint and
any stdout emitted before failure (including gone pages). Protocol/framing
refusals use code 2 and leave the session usable. Cache status replies carry
the same codes and hints as `dctx cache status`. An error reply does not change the server's process exit
status: normal EOF exits 0, transport I/O errors exit 3, and broken stdout exits
quietly with 0. The server performs no piggyback auto-clean.

`pack --for TEXT --json` now emits the facade's context JSON page; its text
member contains the same budgeted context page as plain `pack --for TEXT`.
