# DCX-BUG-01 — 2026-11-07

Base: `eca259da1d7f35d4633fe634a2361c37e6c616ce`. No commit created.

All five bugs were reproduced before their fixes. Nine regression tests were added.
The requested full-workspace Clippy check is blocked by missing desktop system
libraries in this container; no workspace lists, CI, or gates were changed.

## Environment and commands

Working directory: `/path/to/workdir/codex/DCX-BUG-01`.

```sh
export PATH=$HOME/.cargo/bin:$PATH
export RUSTC_WRAPPER=
umask 022
```

`rustc --version`: `rustc 1.99.0 (b940084d7 2026-09-28)` (exit 0).
`cargo --version`: `cargo 1.99.0 (5f94df478 2026-08-27)` (exit 0).
The home Cargo configuration selects `sccache`, which cannot start in this sandbox.
The first `cargo build -p dctx-cli --offline` and a retry with `unset RUSTC_WRAPPER`
both exited 101 (`sccache: Operation not permitted`), before compilation. Setting
`RUSTC_WRAPPER` to the empty string overrides that Cargo configuration; the same
build then exited 0. Subsequent Cargo commands use the environment above.

Logs and command output from this session are under `/tmp/DCX-BUG-01/`.

## 1. Worker startup failure

Before: the injectable worker returned `Refusal::Worker`, but `extract` returned
`Ok(Extraction { completion: Partial(Worker), coverage: Opaque, ... precision:
LineScan })`. That exposed different spans instead of failing. The initial
regression (`worker_spawn_failure_is_an_error_without_degraded_facts`) failed:
0 passed, 1 failed, 30 filtered; exit 101.

After: an injected spawner returning `std::io::ErrorKind::WouldBlock` (EAGAIN)
returns `Err(Refusal::Worker)`, with no partial facts. Real worker startup and join
failures follow this same path. The facade propagates the error as IO, and the
map-parser adapter treats it as parser unavailability rather than bad source
syntax. Syntax/content refusals retain their existing recovery behavior.
The injectable spawner is private; there is no production environment switch.

Exact focused command, before and after:

```sh
cargo test --offline -p dctx-lex-rust --lib worker_spawn_failure
```

After: 1 passed, 0 failed, 30 filtered; exit 0.

A Linux process-level check also forced **every** `pthread_create` in the CLI
child to fail, without changing process-wide resource limits or repository code:

```sh
cat > /tmp/DCX-BUG-01/fail-spawn.c <<'C'
#include <errno.h>
#include <pthread.h>
int pthread_create(pthread_t *thread, const pthread_attr_t *attr, void *(*start)(void *), void *arg) {
    (void)thread; (void)attr; (void)start; (void)arg;
    return EAGAIN;
}
C
cc -shared -fPIC -o /tmp/DCX-BUG-01/fail-spawn.so /tmp/DCX-BUG-01/fail-spawn.c
LD_PRELOAD=/tmp/DCX-BUG-01/fail-spawn.so \
  DCTX_AUTO_CLEAN=0 DCTX_CACHE_DIR=/tmp/DCX-BUG-01/failed-spawn-cache \
  target/debug/dctx-cli map find helper \
  --place crates/dctx-map-parse/tests/fixtures/rust
```

Compiler exit 0; CLI exit **3**, stdout **0 bytes**. Stderr:

```text
❌ Error: map: io: Rust parse worker could not start or failed; check OS thread/process and memory limits
hint: check that the place exists and is readable, then run the query again
```

## 2. `pack --for` on the small fixture crate

Fixture: `crates/dctx-map-parse/tests/fixtures/rust`: one 97-byte Cargo.toml and two
Rust files (799 and 268 bytes), 1,164 bytes total. No fixture files were edited.

```sh
DCTX_AUTO_CLEAN=0 DCTX_CACHE_DIR=/tmp/DCX-BUG-01/cache \
  target/debug/dctx-cli pack crates/dctx-map-parse/tests/fixtures/rust \
  --for helper --budget 103
```

Before (exit 0): the formatter dropped longer lines that did not fit, then kept
shorter later lines. The read section contained **only a closing brace**, with
neither its record header nor its declaration:

```text
## read s:src/support.rs#helper
}
# end body_tokens=76 budget=103 dropped=4
```

After (exit 0): each section keeps a contiguous prefix. A section whose first
line does not fit contributes no title or later source lines. The same find and
neighbor prefix remains, with no orphaned read section:

```text
# end body_tokens=66 budget=103 dropped=5
```

The finished page is 271 bytes / 78 estimated tokens, inside the 103-token cap.
The unit regression also puts `},` and `}` after an oversized source line and
asserts that neither can be admitted after the cut.

A second poor result was reproduced with:

```sh
DCTX_AUTO_CLEAN=0 DCTX_CACHE_DIR=/tmp/DCX-BUG-01/cache \
  target/debug/dctx-cli pack crates/dctx-map-parse/tests/fixtures/rust \
  --for src/lib.rs
```

Before: `terms=0`, an otherwise empty header/end page, exit **1**, despite that
file existing. File names are now kept intact, including short stems such as
`lib.rs`. A matched file selects its module for neighbors/read. After: `terms=1`,
`## find src/lib.rs`, `## neighbors m:src/lib.rs`, and `## read m:src/lib.rs`
with the fixture source in order; exit **0**. This avoids picking an arbitrary
symbol from the file as the task's best hit.

Regression commands:

```sh
cargo test --offline -p dctx-cli --bin dctx-cli pack_for::
cargo test --offline -p dctx-cli --test bug_01
```

Before: pack unit selection 15 passed, 2 failed, 62 filtered, exit 101; CLI
regressions 0 passed, 5 failed, exit 101. Final results appear below.

## 3. Serve from a fixtures directory

On this HEAD, the reported behavior differs from the card's exit-1 observation:
`serve` was **not routed at all**, and exited **2** as `unknown command serve`.
This was reproduced with the built binary from inside the fixture crate.
`read` likewise exited 2 as an unknown command; its implemented spelling was
`map read`.

After: `serve [PATH]` and `serve --mcp [PATH]` route to the existing stdio MCP
server. Omitting PATH uses the current directory. `read` routes to `map read`.
Help and `docs/CLI-EXIT-CODES.md` describe these aliases and their existing exit
contract: server EOF 0, bad arguments 2, inaccessible PATH 3. The server indexes
on tool requests, as `mcp` already does.

Exact manual server check (the relative binary path climbs back to the worktree):

```sh
cd crates/dctx-map-parse/tests/fixtures
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"find","arguments":{"query":"Widget"}}}' \
  | DCTX_AUTO_CLEAN=0 DCTX_CACHE_DIR=/tmp/DCX-BUG-01/after-cache \
    ../../../../target/debug/dctx-cli serve
```

After: exit **0**, JSON-RPC result includes `s:rust/src/lib.rs#Widget`, with no
`isError`. The compiled-CLI regression runs `serve`, `serve --mcp`, and `mcp`
from the actual directory named `fixtures`, asserting this same result.

## 4. Clean read budget boundaries

Before: reading a JSON-like block with a 70-token budget ended as follows:

```text
{
  "first": {
    "name": "short"
  },
[DCTX_TRUNCATED] lines 5-6
```

After: a budget cut selects a whole-line prefix ending on content. Closing-only,
separator-only and blank lines at the boundary move into the continuation:

```text
{
  "first": {
    "name": "short"
[DCTX_TRUNCATED] lines 4-6
```

Those lines are not deleted. The facade regression checks both JSON and text
encodings, CRLF and UTF-8, the exact next-line cursor, `more`, byte limits, and
that prefix plus resumed source equals the original byte for byte. If a resumed
page can fit only closing lines and cannot fit the following content, it returns
`BudgetTooSmall` with an empty sink. Increasing the budget completes the read.
A separate compiled-CLI regression exercises both `map read` and `read`.

Reproducible source and command (70 estimated tokens):

```sh
mkdir -p /tmp/DCX-BUG-01/read-evidence
python3 - <<'PY'
from pathlib import Path
Path('/tmp/DCX-BUG-01/read-evidence/sample.json').write_text(
    '{\n  "first": {\n    "name": "short"\n  },\n  "second": "' + 'x' * 1000 + '"\n}\n'
)
PY
DCTX_AUTO_CLEAN=0 DCTX_CACHE_DIR=/tmp/DCX-BUG-01/evidence-cache \
  target/debug/dctx-cli read sample.json \
  --place /tmp/DCX-BUG-01/read-evidence --budget 70
```

Before using the existing spelling `map read`: exit 0, dangling `},` before the
marker (also captured in the initial failing CLI regression). After: exit 0,
clean prefix and `[DCTX_TRUNCATED] lines 4-6`.

```sh
cargo test --offline -p dctx --test map_read_window
```

Final focused run: 3 passed, 0 failed, 0 ignored; exit 0.
A first test-authoring run used unsupported output format 0 (2 passed, 1 failed,
exit 101); corrected to the public formats 1 and 2. The added continuation
assertion first reproduced a second dangling-only page (0 passed, 1 failed,
2 filtered, exit 101), then passed after requiring a content boundary on resumed
pages too.

## 5. Release changelog path

At this HEAD the stale path was `docs/DOCUMENTS/CHANGELOG.md`
(without a `docs/LEGACY/` prefix). The release recipe's check and repair hint now
both name the current root `CHANGELOG.md`. The release recipe was not executed,
since it creates commits/tags and pushes. The regression inspects the recipe
without running it; it failed on the old path and passes on the current path.
`docs/LEGACY` remains present and untouched.

## Validation

Final required workspace run: **2,245 unit/integration tests + 4 doctests = 2,249
passed**, **0 failed, 0 ignored, 0 measured, 0 filtered**; exit **0**. There were
202 unit/integration result blocks and 36 doctest result blocks. The nine added
regressions all passed.

Count reconciliation: the documented 2,279-test baseline includes the desktop-API
crate (43 `#[test]` cases) and counts unit/integration tests. The requested command
also excludes that crate: **2,279 - 43 + 9 = 2,245** unit/integration tests. The
four doctests bring this run's aggregate to 2,249.

The repository-scale check passed after building 2,326 source files: recall
43/50, no stale files, reopened content reads 0 and reindexes 0. Its debug full
index took 355.87 s; reopened-query p95 was 2.33 s. Performance thresholds were
not enabled or changed.

Final full log: `/tmp/DCX-BUG-01/workspace-test-final.log`.


Exact required test command:

```sh
cargo test --workspace --offline --exclude dctx-desktop --exclude dctx-desktop-api
```

The first attempt exited 101 before running tests: an intermediate new parser
error variant required changing an exhaustive match in the corpus sweep. That
variant was removed in favor of the existing parser-setup error, so no gate or
corpus-sweep code needed edits. A second run was interrupted (exit 130, 1,582
completed tests passed) while final source/test refinements were made. The final
run above was restarted against a stable source tree and completed successfully.

Exact required Clippy command:

```sh
cargo clippy --workspace --all-targets --offline -- -D warnings
```

Exit **101**. The final full-workspace attempt cannot build the desktop native
dependencies: pkg-config cannot find `javascriptcoregtk-4.1`, `gdk-pixbuf-2.0`,
`libsoup-3.0`, or `gdk-3.0` development packages. This is a container prerequisite
failure, not a Rust lint result. An earlier attempt also encountered the
intermediate exhaustive-match compile error described above.

Supplementary check, without modifying workspace membership or gates:

```sh
cargo clippy --workspace --all-targets --offline \
  --exclude dctx-desktop --exclude dctx-desktop-api -- -D warnings
```

Exit **0**, zero warnings, including after the final source changes.

```sh
cargo test --offline -p dctx-cli --test bug_01 --bin dctx-cli
```

Final focused run: **79 binary unit tests + 5 CLI regressions passed**, 0 failed,
0 ignored; exit **0**. The worker unit test and all 3 read-window tests also pass.

```sh
git diff --check
```

Exit 0. All prohibited paths remain unedited; no Cargo workspace list, CI or gate
was changed, and no commit was made. DIDN'T obtain a clean full-workspace Clippy result: the native desktop development
libraries are missing. All five fixes, their regressions, the requested workspace
test run, and supplementary non-desktop Clippy are complete.
