<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# npm transport and release packages

`dctx/` contains the dependency-free Node.js RPC client and CLI transport.
Each `cli-*` directory is an optional OS/CPU constrained package containing the
native binary. Executables are generated and ignored by git. There are no
installation hooks or binary downloads. The owner publishes manually from a Mac,
using the GitHub release assets; CI does not publish npm packages.
Available on npm from version 1.3.1, `@tejl/dctx` pins `@tejl/dctx-<platform>` optional dependencies.
Then `npm install @tejl/dctx` installs the shim and the matching native package.
See [the package API](dctx/README.md) for resolution and handshake behavior.
See [npm verification](../../docs/VERIFYING.md#verify-npm-packages) for binary
checksum checks. There is no npm provenance attestation.

From the repository root:

```sh
# Regenerate wire declarations after a Rust RPC schema update.
node packaging/npm/scripts/generate-types.mjs
node packaging/npm/scripts/generate-types.mjs --check
cargo build --locked -p dctx-cli
DCTX_TEST_BIN="$PWD/target/debug/dctx-cli" node --test packaging/npm/test/*.test.cjs
# Run the package suite through npm (set DCTX_TEST_BIN for real-engine parity).
(cd packaging/npm && npm test)
# Validate versions, syntax and schema declarations; write a local tarball only.
node packaging/npm/scripts/build-pack.mjs
```

On Windows, use `dctx-cli.exe` and run pack/tests through `npm exec -- node ...`
to supply npm's JavaScript entry point without invoking a command shell.
The default pack destination is `packaging/npm/artifacts`; pass another directory
as the first script argument. No build or test command publishes anything.
The local packed tarball omits `optionalDependencies` so `npm install <tarball>`
does not resolve platform packages from the configured registry.
Users of that tarball set `DCTX_BIN` (see
[the package API](dctx/README.md)).
CI checks Node 18 and 24 on Linux, macOS and Windows, every RPC verb against CLI
stdout/status, transport lifecycle/refusals, TypeScript consumers and actual
packed contents. The package suite also rejects `preinstall`, `install`,
`postinstall` and `prepare` scripts in every npm package manifest.
The output string is opaque, including errors and trailing
newlines. Tests discover the verb set from the schema to prevent coverage drift.

To fill all six packages from five local release archives and `SHA256SUMS`:

```sh
python3 tools/release/prepare-npm.py v1.3.1 artifacts/
(cd packaging/npm/dctx && npm pack --ignore-scripts --dry-run)
python3 tools/release/test-packaging.py
```

The preparation script verifies every archive before writing packages, sets
versions from the tag, declares the five platform packages as optional
dependencies of `@tejl/dctx`, and copies LICENSE/NOTICE. These prepared packages are
used for registry publication. To install them locally without resolving optional
dependencies from a registry, name both tarballs explicitly:
`npm install --omit=optional --ignore-scripts ./tejl-dctx-<version>.tgz ./tejl-dctx-<os>-<arch>-<version>.tgz`.
Archive names use full Cargo target triples. The Linux x64 packaging test exercises all six tarballs,
checksum rejection, CLI argument forwarding, exit status and signals without
publishing. The npm transport CI job requires no tokens.
