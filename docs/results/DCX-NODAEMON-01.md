<!-- SPDX-License-Identifier: Apache-2.0 -->
# DCX-NODAEMON-01

**Result: complete; every required gate passed.**

Base revision: `203e5cd33fca151569e1f62ec6b6e38fff57889f`. No commit created.

The optional daemon is removed in accordance with the 2026-11-08 owner decision
and SPEC's non-goals. The transport crate is deleted, so the workspace's
`crates/*` member glob no longer includes it. Its CLI dependency and lockfile
entries, CLI implementation, internal entry point, command, environment/config
handling, auto-start, output interception, lifecycle tests, and guide are gone.
Map, read, and both pack routes call their existing local implementations.
CLI/RPC confinement coverage remains in the security tests.

The only edit under `crates/dctx-cli/src/host/` deletes the daemon-only
`set_session_auto_clean` helper. Core session/index code and host open/rule code
are unchanged. The per-invocation stamp path remains the performance path; this
card makes no new latency claim. `docs/archive/` and SPEC are unchanged.

Getting-started and security prose now describe the remaining implementation.
The plugin wire contract consistently calls its session-bound process a worker.
Existing no-daemon statements in WHY-DCTX, the RPC and cache contracts, and
package READMEs remain accurate and are retained.

Website captures and generated outputs were rebuilt with the repository tools.
No website generator, template, stylesheet, script, font, or image was edited.
THIRD_PARTY.md was regenerated: its 127-package contents are byte-identical,
because the remaining dependency graph still reaches those external packages.

## Required gates

Commands ran with `umask 022`, the installed Rust toolchain on `PATH`,
`CARGO_TARGET_DIR="$PWD/target"`, and `TMPDIR="$PWD/target/tmp"`.
Release and subsequent Cargo checks use `CARGO_HOME="$PWD/target/cargo-home"`
with the installed registry cache available offline. Raw logs are under
`target/card-logs/`.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | PASS, exit 0 |
| `cargo clippy --workspace --exclude dctx-desktop --exclude dctx-desktop-api --all-targets --locked -- -D warnings` | PASS, exit 0 |
| `cargo test --workspace --exclude dctx-desktop --exclude dctx-desktop-api --locked` | PASS, exit 0; 2,426 passed, 0 failed, 0 ignored across 261 suite summaries |
| `python3 tools/engine-contract/check_imports.py` | PASS, exit 0; 18 files, only allowlisted engine APIs |
| `bash scripts/spdx_check.sh` | PASS, exit 0; 39 crates, 9 desktop sources |
| `python3 scripts/third_party.py --out THIRD_PARTY.md` | PASS, exit 0; 127 packages, no content change |
| `python3 scripts/third_party.py --out target/tp.md && diff -u THIRD_PARTY.md target/tp.md` | PASS, exit 0; no difference |
| `cargo build --release --locked -p dctx-cli && python3 tools/engine-contract/parity.py target/release/dctx-cli` | PASS, both exit 0; 2 cases, 3 surfaces, 6 exact byte comparisons |
| `grep -rniw daemon --include=*.rs crates` | PASS, exit 0; exactly the two pre-existing `//! No daemon` comments in dctx-adapter-cache |
| `git diff --check` | PASS, exit 0 |

## Regeneration and additional checks

The website dependencies were installed from the pinned
`tools/website/requirements.txt` into `target/website-venv`.

```sh
DCTX_BIN=target/release/dctx-cli target/website-venv/bin/python tools/website/capture.py
python3 scripts/render_readme.py
DCTX_BIN=target/release/dctx-cli target/website-venv/bin/python tools/website/capture.py --llms
target/website-venv/bin/python tools/website/build.py
DCTX_BIN=target/release/dctx-cli target/website-venv/bin/python tools/website/check.py --working-tree
```

The capture, README renderer, llms capture, and website build passed with exit 0.
The build produced 45 pages and 191 files. README.md required no generated change.
Website validation passed with exit 0: 191 files, 45 HTML pages, 22 Markdown
twins, exact capture/rebuild checks, and zero errors. Its standard warning notes
that no external publish deny-list was supplied. A separate call to the same
`check.deny_list` scanner with a target-local list of sandbox paths and host
identifiers also passed with zero matches (`target/card-logs/website-privacy.log`).

A Python comparison against `git show HEAD:tools/website/data/demo.json` passed:
all 40 non-help records retain identical stdout, stderr, exit status, and recorded
metadata. The remaining help record differs only by deleting the command line
and explanatory daemon line. The committed engine goldens were not modified. The workspace test
`sixteen_operations_are_byte_identical_across_transports` also passed, along with
the retained CLI/RPC symlink and traversal confinement test.

`python3 target/card-smoke.py` passed against the release binary. It checks that
`daemon status` and `--internal-daemon` return usage exit 2, help omits the daemon,
and 12 exact stdout/stderr/status comparisons for map find, read, pack, and
pack-for ignore legacy enabled/disabled/invalid daemon environment values and an
old enabled configuration. The old config is unchanged and no socket is created.
The preliminary smoke fixture selected zero task-context terms; replacing its
term with the sample's known `greet` symbol made the positive control valid.

A scope audit passed: removed directories and guide are absent, workspace
metadata contains no transport crate, archived docs and SPEC are unchanged,
reserved core/host files are unchanged, and website assets/design sources are
unchanged. Remaining nonhistorical daemon mentions describe its absence or
Python thread lifecycle, rather than a dctx feature.
