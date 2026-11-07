<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Runnable CLI quickstart

For in-process Rust integration, see the [compiled library example](library/README.md).

Requires zsh and a recent stable Rust toolchain with Cargo (the workspace includes
Rust 2024 crates). Run from the repository root:

```zsh
zsh examples/quickstart.sh
```

The script uses `cargo run --locked -p dctx-cli --bin dctx-cli` with the repository
manifest, so it also works from another directory. Cargo may download dependencies
on the first run. It uses the default CLI features and supports `CARGO_TARGET_DIR`.
The executable and package are both named `dctx-cli`; no `dctx` alias is required.

It creates a temporary Rust fixture, then:

1. Prints the CLI version.
2. Packs only `.rs` files into an explicit output file and checks its source text.
3. Outlines `main.rs` and searches for the `greeting` function.
4. Reads `m:main.rs` as JSON and checks that the returned source includes the greeting.

The fixture, output, CLI configuration and map cache live in the temporary directory
and are removed on exit. The script disables automatic cache cleaning. Cargo build
artifacts remain in Cargo's target directory. All dctx calls use the synthetic
fixture, so the example does not scan your checkout.

The equivalent commands with a built CLI are:

```zsh
./target/debug/dctx-cli pack /absolute/path/to/fixture --ext rs --output /tmp/pack.txt
./target/debug/dctx-cli map map main.rs --place /absolute/path/to/fixture --depth 2
./target/debug/dctx-cli map find greeting --place /absolute/path/to/fixture --limit 5
./target/debug/dctx-cli map read m:main.rs --place /absolute/path/to/fixture --json
```

Both text `read` and `read --json` include the source body; use JSON's `source_utf8`
field when consuming it in a script. Map results describe structural facts; they do not establish
full semantic call resolution. See the [documentation index](../docs/README.md),
[map guide](../docs/guides/MAP-MODE.md) and `dctx-cli map --help` for details.
