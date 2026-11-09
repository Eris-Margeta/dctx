<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Testing

CI (`.github/workflows/ci.yml`) runs on pushes to `main`, all pull requests
(including forks), and manual dispatch. It is one fail-fast chain, cheapest stage
first:

1. **quality**: one Ubuntu/stable job with the format, lint, docs and release gates
   described below.
2. **linux**, then **macos**, then **windows**: each calls the reusable
   [`tests.yml`](../.github/workflows/tests.yml) workflow for that stage's runners.
3. **packages**: after Windows passes, five called workflows run in parallel:
   [`npm.yml`](../.github/workflows/npm.yml) (Node transport, CLI parity and tarball
   contents on Ubuntu, macOS and Windows with Node 18 and 24; packs the client
   without publishing), [`go.yml`](../.github/workflows/go.yml) (generated API,
   race and every-verb CLI parity tests; Go 1.23 and stable on Linux, stable on
   macOS and Windows), [`python-packages.yml`](../.github/workflows/python-packages.yml)
   (transport tests on Python 3.11 and 3.13, native platform wheels on their own
   hosts, and an offline install from a local wheelhouse),
   [`performance.yml`](../.github/workflows/performance.yml) (synthetic CLI and
   catalog benchmarks under an instruction counter, checked against the committed
   baseline) and [`fuzz-smoke.yml`](../.github/workflows/fuzz-smoke.yml) (bounded
   parser property targets with a 180-second budget).
4. **deploy-site**: only on pushes to `main`, after every package workflow passes,
   [`deploy-site.yml`](../.github/workflows/deploy-site.yml) publishes the website
   when the commit changed `website/`, `tools/website/`, `docs/` or `CHANGELOG.md`
   and the deployment secrets are configured.

A stage starts only when every earlier stage passed, and a failing step stops its
job. Inside a stage, the first failing OS/toolchain combination cancels the rest
of the stage (`fail-fast: true`) and every later stage, so a red commit never pays
for the more expensive hosts. Manual dispatch can start at one stage, stop after
another (`start`/`until`) and pass a test-name `filter`, to iterate on one
platform failure without rerunning the whole chain. The package, performance and
fuzz workflows can also be dispatched on their own. The release workflows
(`release.yml` on `v*` tags, the manually dispatched `reproducible.yml`, and
`release-desktop.yml` on `desktop-v*` tags) are separate from this chain.

Each test-stage combination below runs `cargo test --workspace --locked --exclude
dctx-desktop --exclude dctx-desktop-api`, including doctests. Logs show the
resolved compiler version and native host architecture. Test jobs allow 90
minutes; the Ubuntu quality job allows 60 minutes.

| GitHub-hosted runner | Requested architecture | Toolchains |
| --- | --- | --- |
| `ubuntu-24.04` | x64 | stable, MSRV |
| `macos-15` | ARM64 | stable, MSRV |
| `macos-15-intel` | x64 | stable, MSRV |
| `windows-2022` | x64 | stable, MSRV |

The workflow reads `workspace.package.rust-version` (or root `package.rust-version`)
from `Cargo.toml`. The workspace declares MSRV **1.91**.
Existing unit tests use [`Duration::from_mins`](https://doc.rust-lang.org/std/time/struct.Duration.html#method.from_mins),
stable since Rust 1.91; a local Rust 1.90.0 run fails to compile those tests.
Desktop packages are excluded.

One additional Ubuntu/stable job checks formatting, Clippy with `-D warnings`,
Rust documentation with `-D warnings`, licence manifests and `THIRD_PARTY.md`, the
existing `deny.toml` licence policy, the website (including release-binary capture
drift), playground tests, and the release gates with `strace`. These checks require
no exclusive self-hosted facility, so they replace the former self-hosted job.
The quality job also runs cargo-audit, the release supply-chain script tests,
workflow validation and CLI/MCP/RPC golden byte parity. Its steps run in order and
the first failing step stops the job and the chain.
All actions use full commit SHAs; caches are separated by OS/toolchain/job and
saved only on `main`. Workflow permissions are `contents: read`; newer PR runs
cancel older runs for the same PR.

Run the same tests locally from the repository root with Rustup and Python 3.11+
(CI uses Python 3.12). On Windows, use Git Bash for shell examples and `python`
instead of `python3` if necessary:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
umask 022
rustup toolchain install stable --profile minimal --component rustfmt --component clippy
rustup toolchain install 1.91.0 --profile minimal
python3 - <<'PY'
import subprocess
results = []
for toolchain in ("stable", "1.91.0"):
    print(f"Testing {toolchain}", flush=True)
    result = subprocess.run(
        ["cargo", f"+{toolchain}", "test", "--workspace", "--locked",
         "--exclude", "dctx-desktop", "--exclude", "dctx-desktop-api"],
        close_fds=True,
    )
    results.append(result.returncode)
raise SystemExit(int(any(results)))
PY
cargo +stable fmt --all -- --check
cargo +stable clippy --workspace --locked --exclude dctx-desktop --exclude dctx-desktop-api --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo +stable doc --workspace --locked --exclude dctx-desktop --exclude dctx-desktop-api --no-deps
bash scripts/spdx_check.sh
python3 scripts/third_party.py --out target/THIRD_PARTY.md
diff -u THIRD_PARTY.md target/THIRD_PARTY.md
# Only when deny.toml exists:
cargo +stable install cargo-deny --version 0.20.2 --locked
cargo +stable deny --locked check licenses
cargo +stable build --release --locked -p dctx-cli
python3 -m venv tools/website/.venv
tools/website/.venv/bin/python -m pip install -r tools/website/requirements.txt
DCTX_BIN="$PWD/target/release/dctx-cli" tools/website/.venv/bin/python tools/website/check.py
cargo +stable test --locked --manifest-path tools/playground/Cargo.toml
# Linux, with strace installed:
scripts/release-check.sh --quick "$PWD/target/release/dctx-cli"
```

On Windows the website venv interpreter is `tools/website/.venv/Scripts/python.exe`
and the release binary ends in `.exe`. Use a full git history for website document
date checks. `DCTX_DENY_LIST` enables the separate publish deny-list check when its
external policy file is available; it is not supplied by this workflow.

Every CLI integration test must create CLI child commands through the shared
`crates/dctx-cli/tests/common/mod.rs` helper (`mod common;`).
That helper owns a fresh temporary home/config/cache environment for each test
and prevents inherited user configuration or caches from changing results. Keep
the temporary environment alive until every child has exited. Do not spawn the
CLI directly or mutate the parent process environment. CI closes inherited file
descriptors before launching Cargo as required by native confinement tests; this
does not replace per-test environment isolation. The helper is shared by the CLI integration tests.
