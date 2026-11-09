<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# npm transport and release packages

`dctx/` contains the dependency-free Node.js RPC client and CLI transport.
Each `cli-*` directory is an optional OS/CPU constrained package containing the
native binary. Executables are generated and ignored by git. There are no
installation hooks or binary downloads. Registry publication is pending; use
[GitHub Releases](https://github.com/Eris-Margeta/dctx/releases) for native archives.
See [the package API](dctx/README.md) for resolution and handshake behavior.

From the repository root:

```sh
# Regenerate wire declarations after a Rust RPC schema update.
node packaging/npm/scripts/generate-types.mjs
node packaging/npm/scripts/generate-types.mjs --check
cargo build --locked -p dctx-cli
DCTX_TEST_BIN="$PWD/target/debug/dctx-cli" node --test packaging/npm/test/*.test.cjs
# Validate versions, syntax and schema declarations; write a local tarball only.
node packaging/npm/scripts/build-pack.mjs
```

On Windows, use `dctx-cli.exe` and run pack/tests through `npm exec -- node ...`
to supply npm's JavaScript entry point without invoking a command shell.
The default pack destination is `packaging/npm/artifacts`; pass another directory
as the first script argument. No build or test command publishes anything.
The packed tarball omits `optionalDependencies`: the `@dctx/cli-*` names are not
registered yet, and declaring them would make `npm install <tarball>` resolve
them from the configured registry. Users of that tarball set `DCTX_BIN` (see
[the package API](dctx/README.md)).
CI checks Node 18 and 24 on Linux, macOS and Windows, every RPC verb against CLI
stdout/status, transport lifecycle/refusals, TypeScript consumers and actual
packed contents. The output string is opaque, including errors and trailing
newlines. Tests discover the verb set from the schema to prevent coverage drift.

To fill all six packages from five local release archives and `SHA256SUMS`:

```sh
python3 tools/release/prepare-npm.py v1.3.0 artifacts/
(cd packaging/npm/dctx && npm pack --ignore-scripts --dry-run)
python3 tools/release/test-packaging.py
```

The preparation script verifies every archive before writing packages, sets
versions from the tag, declares the five platform packages as optional
dependencies of `dctx`, and copies LICENSE/NOTICE. Until the names are
registered, install those tarballs only with `--omit=optional` and the
matching platform tarball named explicitly:
`npm install --omit=optional --ignore-scripts ./dctx-<version>.tgz ./dctx-cli-<os>-<arch>-<version>.tgz`.
Archive names use full Cargo target triples. The Linux x64 packaging test exercises all six tarballs,
checksum rejection, CLI argument forwarding, exit status and signals without
publishing. See [the maintainer runbook](../../docs/RELEASING.md) for the existing
credential-gated release process. The npm transport CI job requires no tokens.
