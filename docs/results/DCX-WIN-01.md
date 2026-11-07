# DCX-WIN-01 — 2026-11-07

Base: `e0c8f25920803136dacdb2d4b45bd9c2b019a54b`. No commit created.

## Fix and audit

The reported Windows failure came from the test environment: the global rule
fixture was written under `config/dctx/ignore`, but the subprocess received only
`XDG_CONFIG_HOME`, which the Windows rule resolver does not use. Consequently the
rule edit never reached the map generation key.

Added `crates/dctx-cli/tests/common/mod.rs` and routed all 14 existing cleared
CLI command harnesses through its `isolated_dirs` helper, including `bug_01`
and the pack-help harness. There was no existing CLI common test-support module.
The helper creates each directory and sets:

| Environment variable | Fixture directory |
| --- | --- |
| `HOME`, `USERPROFILE` | `home` |
| `XDG_CONFIG_HOME`, `APPDATA` | `config` |
| `XDG_CACHE_HOME` | `cache` |
| `XDG_DATA_HOME` | `data` |
| `LOCALAPPDATA` | `local-app-data` |
| `TMPDIR`, `TMP`, `TEMP` | `tmp` |

Command-specific cache overrides, plugin paths and PATH remain in their harnesses.
Windows cache/data share the local application-data base; Windows roaming config
and Unix config both point at the actual global-rule fixture.

Audit command, run before and after the change:

```sh
rg -n 'XDG_CONFIG_HOME|LOCALAPPDATA|APPDATA|XDG_CACHE_HOME|XDG_DATA_HOME|env_clear' crates
```

Also checked Rust sources under `plugins` and `apps` for `env_clear`. The only
non-CLI occurrence is the production `NoSecretEnvironment` plugin bootstrap
policy. Its security contract deliberately allows only `LANG` and `LC_ALL`;
it is not a directory-isolating test harness and was left intact.

There is no tracked `compat.rs` at this HEAD (`git ls-files '*compat*'` finds
only `crates/dctx-pack/tests/compatibility.rs`). Reviewed the active implementations
in `crates/dctx-cli/src/host/rules.rs` and `host/cache.rs` instead:

- Windows config resolution now reports `APPDATA is not set or is empty` instead
  of silently skipping global rules or resolving an empty value relative to cwd.
  The error propagates through project rule loading, including omit-only mode.
- Windows cache resolution already errored for an absent `LOCALAPPDATA`; it now
  rejects an empty value too, with `LOCALAPPDATA is not set or is empty`.
- Directory environment values use `var_os`, preserving native path values.
  Unix config still uses XDG then HOME; empty XDG config falls back to HOME.
  Platform cache suffixes and explicit `DCTX_CACHE_DIR` behavior are preserved.

The config and cache resolvers accept injected environment lookups. Four unit
regressions exercise Windows selection, missing/empty variables, and Unix config
fallback from provided maps on every platform, without mutating process globals.
An additional integration regression inspects the actual shared helper's command
environment, including `APPDATA=config`, `LOCALAPPDATA`, and created directories.
The existing ignore/edit/rebuild integration test continues to cover the original
behavior through the compiled CLI. `CHANGELOG.md` records the fix under
`[Unreleased] / ### Fixed`.

## Validation

Validation ran in the Linux internal sandbox. Native Windows execution is
not available here; Windows resolver branches and the harness mapping are tested
on Linux with injected maps.

Toolchain: `rustc 1.99.0 (b940084d7 2026-09-28)`;
`cargo 1.99.0 (5f94df478 2026-08-27)`; `umask 022`.
The desktop pkg-config prerequisites are installed: javascriptcoregtk-4.1 2.54.0,
gdk-pixbuf-2.0 2.42.12, libsoup-3.0 3.6.5, gdk-3.0 3.24.49.

Cargo builds, temporary files, isolated home/config/cache/data, Cargo lock state and
logs live under this worktree's ignored `target/` directory. The local Cargo home
uses the preinstalled registry through a read-only source link. The environment
setup used for each command is recorded in `target/card/env.sh`; it places
`/home/user/.cargo/bin` on PATH and sets `RUSTC_WRAPPER=`.

```sh
. target/card/env.sh
cargo test --offline -p dctx-cli
cargo test --workspace --offline --exclude dctx-desktop --exclude dctx-desktop-api
cargo clippy --workspace --all-targets --offline -- -D warnings
git diff --check
```

CLI package run: **210 passed, 0 failed, 0 ignored**, exit **0** (including
all five new regressions and the original ignore/edit/rebuild test).
Log: `target/card/cli-test-final.log`.

The first CLI compile exited 101 before tests ran: a bulk edit inserted `mod
common` inside two introductory comments containing “reuse”. Those insertions
were corrected; the final CLI run above uses the corrected sources.
Initial log: `target/card/cli-test.log`.

Required workspace run: **2,229 unit/integration tests + 4 doctests = 2,233
passed**, **0 failed, 0 ignored, 0 measured, 0 filtered**; exit **0**.
This matches the card's **2,228 baseline + 5 new regressions**.
Log: `target/card/workspace-test.log`.

The repository-scale test passed: 2,331 files parsed, recall 41/50, no stale
files, reopened content reads 0, reindexes 0. Debug full indexing took
1,423.856 s; reopened-query p95 was 8.473 s. Optional performance thresholds
were not enabled or changed.

Required full-workspace Clippy run, including desktop and all targets: exit
**0**, **zero warnings**. Log: `target/card/clippy-final.log`.

The first Clippy attempt exited 101 because Tauri's configured `../dist` did
not exist; all native desktop libraries were available. Bun is unavailable in
this sandbox. For the final Rust check, prepared the same ignored asset
placeholder documented by the existing `tools/desktop-check.sh`:

```sh
umask 022
mkdir -p apps/dctx-desktop/dist
printf '%s\n' '<!DOCTYPE html><title>dctx desktop check placeholder</title>' > apps/dctx-desktop/dist/index.html
. target/card/env.sh
cargo clippy --workspace --all-targets --offline -- -D warnings
```

This is a Rust lint check, not a frontend build. No gate or Tauri configuration
was edited. The initial attempt is logged in `target/card/clippy.log`. Tauri
also regenerated two tracked schema JSON files; both were restored to HEAD
after validation, leaving only the card changes in the diff.

`git diff --check`: exit **0**. Workspace manifests, lockfile, CI and gate paths
were checked for changes and remain unedited.

No Cargo workspace list, CI configuration, or gate was edited. No commit was made.

Reply for tester: Fixed `APPDATA=config` through one shared helper for all
14 cleared CLI harnesses, with `LOCALAPPDATA` supplied consistently too. The
original rule-edit test and five new cross-platform regressions pass. Both
required Linux gates are green (2,233 tests; full Clippy, zero warnings). Please
rerun `cargo test --locked --release -p dctx-cli` on Windows 10.
