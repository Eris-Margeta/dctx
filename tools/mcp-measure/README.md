<!-- SPDX-License-Identifier: Apache-2.0 -->
# Measure MCP transport overhead

From the repository root, using Python 3.11+ and a built binary:

```sh
RUSTC_WRAPPER= cargo build -p dctx-cli --release --locked --target-dir target
python3 tools/mcp-measure/measure.py target/release/dctx-cli
python3 tools/mcp-measure/measure.py target/release/dctx-cli --format json
```

Use `target/release/dctx-cli.exe` on Windows, or supply an installed `dctx`
binary's absolute path. No Python packages, model credentials or network
access are required. The [guide](../../docs/guides/MCP.md) explains the results;
[sample.json](sample.json) records the documented run, not a performance gate.

The script creates a deterministic 26-file Rust fixture and private cache
under a temporary directory in the current directory, and removes both on
exit. It strips inherited `DCTX_*` settings and disables automatic cleanup
for its child processes. It does not query your project or ambient dctx cache.
Use `--scratch-dir PATH` to choose another existing writable parent.

It measures actual initialize/tools-list reply lines for full and lean
profiles, compact tool definitions and input schemas, CLI/MCP read byte counts,
one first query with an empty dctx cache, warm queries and a pipelined burst
on one stdio process. It checks exact decoded read equality in text and facade
JSON formats, find/read cursor advancement, an absolute line window, `isError`
with text and session survival after an argument error. A mismatch fails the
run instead of printing misleading results.

`--samples 7` sets the warm sample count; `--burst 8` sets outstanding finds
before a queued ping; `--timeout 60` bounds each response and subprocess wait.
The byte counts include the serialization conventions described in the guide.
Token estimates are bytes / 4; no tokenizer is used. Timings use
`time.perf_counter()` and include Python/pipe overhead. The first-query timing
is one sample, not a median, and does not flush OS caches. Finish competing
builds before measuring and repeat runs to assess variability. Serial response
order alone is not proof of serial execution; the guide also cites the server
loop. Editor/client acceptance and ACP handshakes require separate verification.
