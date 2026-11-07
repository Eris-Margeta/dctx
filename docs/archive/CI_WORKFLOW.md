Historical design record; superseded by [the current documentation](../TESTING.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# CI workflows

Three workflows live in `.github/workflows/`. This file describes exactly what they run.

## ci.yml: every push and pull request to `main`

One job, `rust` ("Clippy and tests"), on a self-hosted Linux x64 runner labelled `dctx`. Pull requests from forks
are skipped, so fork code never runs on that machine. Timeout: 60 minutes. The desktop crate (`dctx-desktop`) is
excluded from cargo steps, because the Linux runner has no Tauri system libraries (see docs/STYLE.md).

| Step | Command |
|---|---|
| Toolchain | print `rustc`, `cargo`, `cargo clippy` and `cargo nextest` versions |
| Licence headers and manifests | `bash scripts/spdx_check.sh` (every crate declares Apache-2.0; LICENSE and NOTICE present) |
| Third-party licences | `python3 scripts/third_party.py --out $RUNNER_TEMP/THIRD_PARTY.md`, then `diff` against the committed `THIRD_PARTY.md` (fails on a licence outside the allow-list or a stale file) |
| Third-party npm licences | `python3 scripts/npm_licenses.py --check` (checks npm licence expressions and that `THIRD_PARTY_NPM.md` is current) |
| Clippy | `cargo clippy --workspace --exclude dctx-desktop --all-targets -- -D warnings` (two allowed lints) |
| Tests | `cargo nextest run --workspace --exclude dctx-desktop --no-fail-fast`, started with inherited descriptors closed |
| Release build | `cargo build --release --locked -p dctx-cli` |
| Release gates | `scripts/release-check.sh --quick $CARGO_TARGET_DIR/release/dctx-cli` (code-scan, determinism, unrelated-edit, network) |

There is no `cargo fmt` step: rustfmt is not adopted (docs/STYLE.md).

The same gates run locally with one command: `scripts/release-check.sh` (cargo build, test and clippy, then the
gates above). It adds `~/.cargo/bin` to `PATH` itself, so it also works from non-login shells.

## release.yml: tags `v*`

1. `build` (matrix, fail-fast off): `x86_64-unknown-linux-gnu` (ubuntu-24.04), `x86_64-apple-darwin`
   (macos-15-intel), `aarch64-apple-darwin` (macos-15), `x86_64-pc-windows-msvc` (windows-2022). Each builds the CLI
   in release mode, smoke-tests it, and uploads a `.tar.gz` (Unix) or `.zip` (Windows) artifact.
2. `release` (ubuntu-24.04): downloads all artifacts, checks that the tag matches `VERSION` and
   `tools/sync-version.sh --check`, extracts this version's notes with `tools/extract-changelog.sh`, and creates the
   GitHub release with the artifacts attached (marked prerelease for alpha/beta/rc tags).

## release-desktop.yml: tags `desktop-v*`

One `release` job, matrix macOS arm64, macOS x64, Windows x64 and Linux x64. Steps: checkout, set up Bun and Rust,
install the Linux WebKit/GTK packages (Linux only), install and build the frontend, build the Tauri app, collect the
platform bundles and upload them to the release.
