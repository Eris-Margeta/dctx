<!-- SPDX-License-Identifier: Apache-2.0 -->
# dctx language packages

Python (pip), Node.js (npm), and future Go, Java and .NET packages use the
[versioned engine stdio protocol](../docs/protocol/README.md). The `dctx` binary
is the single source of truth. Packages start the binary, pass arguments,
stream its output and expose host-language idioms.

The [engine philosophy](../docs/PHILOSOPHY.md) defines the binding wrapper
allow/deny list and CI parity requirements. Packages must not implement engine
operations or parse pack text. The former PyO3/NAPI/FFI binding sketches in the
[archived dossiers](../docs/archive/dossiers/dctx-bindings/README.md) are
historical; they do not define the maintained package architecture.

The maintained Python RPC transport lives in [`packages/python`](../packages/python/README.md),
including typed API generation, local binary discovery, platform wheel builds,
CLI byte parity and offline installation tests.
For direct Rust use, call the `dctx` facade in `crates/dctx`.
