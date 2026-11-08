<!-- SPDX-License-Identifier: Apache-2.0 -->
# DCX-MAC-01 — symlinked directory ancestors

Date: 8 November 2026. Base revision: `da25f2c346fb6555fc44c3f85c011733bd653739`.
Validation host: Linux x86-64; Rust 1.99.0. No commit created.

## Change

Resolve trusted directory boundaries before passing child paths to the strict
filesystem reader: global configuration for the CLI and filesystem source,
cache policy ancestors, the map-query database directory, and explicitly selected
template/bundle directories. Project roots already resolve at `FsSource::open`.
Template listing resolves its selected directory once.

`StdFs` and the engine's descriptor-relative no-follow walks retain their checks.
Cache policy loading validates the original cache root with `FsLedgerStore`
before resolving its ancestors; the cache root and policy leaf remain no-follow.
The cache adapter's existing ownership, permission and trusted-ancestor rules are
unchanged. Low-level disk-backed port tests now create fixtures beneath the
canonical temporary directory, matching those APIs' path contract.

Regressions cover map find/read/pack through a linked project ancestor, XDG
configuration through a linked ancestor, a symlinked `~/.config`, trusted cache
ancestors, explicit-file byte limits, and refusal of project file/directory links,
linked rule files, linked cache roots and linked policy files. No assertions,
thresholds or security checks were weakened, and no skips were added.

## Commands and results

The installed Rust toolchain was added to `PATH`. Cargo output was explicitly
kept in the worktree. All commands used `umask 022`.

```sh
umask 022
export CARGO_TARGET_DIR="$PWD/target"
mkdir -p "$PWD/target/real-tmp"
chmod 700 "$PWD/target/real-tmp"
ln -sfn "$PWD/target/real-tmp" "$PWD/target/link-tmp"
```

The pre-fix reproduction command below produced `Not a directory (os error 20)`
failures in filesystem rules and CLI operations. That diagnostic run was stopped
after reproducing the failures. An initial patched compilation exposed a shared
helper visibility error. A subsequent full-suite attempt caught a database path
identity regression in `Index::save`; canonicalization was narrowed to the legacy
read, preserving the original save-path check. Both issues were corrected before
the final gate runs. Earlier partial runs are not counted as completed gates.

`TMPDIR="$PWD/target/link-tmp" cargo test --locked -p dctx-map-query --test pipeline --test legacy`
passed all seven tests after that correction, including an explicit linked-cache
regression that verifies save-path identity and refusal of a linked legacy file.

One later attempt hit two pre-existing freshness tests whose newly written files
had aged beyond their two-second racy window before observation.
`TMPDIR="$PWD/target/link-tmp" cargo test --locked -p dctx --lib session::reuse::tests::stamp`
passed all five tests unchanged in isolation. No timing assertion, threshold or
test fixture was changed for that retry; the full suite was restarted.

| Command | Final result |
| --- | --- |
| `TMPDIR="$PWD/target/link-tmp" cargo test --workspace --locked --no-fail-fast --exclude dctx-desktop --exclude dctx-desktop-api` | PASS; exit 0; zero failures; 807.06 s |
| `env -u TMPDIR cargo test --workspace --locked --no-fail-fast --exclude dctx-desktop --exclude dctx-desktop-api` | PASS; exit 0; zero failures; 675.78 s |
| `cargo fmt --all --check` | PASS; exit 0; 3.75 s |
| `cargo clippy --workspace --exclude dctx-desktop --exclude dctx-desktop-api --all-targets --locked -- -D warnings` | PASS; exit 0; 2.25 s |
| `cargo build --release --locked -p dctx-cli` | PASS; exit 0; 10.42 s |
| `python3 tools/engine-contract/warm_read.py target/release/dctx-cli` | PASS; exit 0; 0.25 s |

Both workspace suites reported 2,428 passed tests and zero failures. The warm-read
syscall contract measured zero cache opens, three target opens, and zero unrelated
source opens/reads across 32 branches. Its bounds were unchanged.

Full final gate logs are local artifacts under `target/DCX-MAC-01-*.log`;
`target/DCX-MAC-01-gates.json` records each final exit status and duration.
The symlinked-temp reproduction exercises the ancestor-resolution failure on
Linux; macOS itself was not run in this environment.
