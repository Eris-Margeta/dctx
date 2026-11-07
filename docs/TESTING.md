<!-- SPDX-License-Identifier: Apache-2.0 -->
# Testing

CI runs on pushes to `main`, all pull requests (including forks), and manual dispatch.
Each combination below runs `cargo test --workspace --locked --exclude dctx-desktop
--exclude dctx-desktop-api`, including doctests. Matrix failures do not cancel other
combinations. Logs show the resolved compiler version and native host architecture.
Test jobs allow 90 minutes; the Ubuntu quality job allows 60 minutes.

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
Independent quality checks continue after a failure, preserving the failed job
status while exposing the other results; dependent checks require their setup.
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
