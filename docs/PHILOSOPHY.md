<!-- SPDX-License-Identifier: Apache-2.0 -->
# The binary is everything

The dctx binary is the single source of truth, even with chunked files. Every
byte dctx emits is produced by the engine. CLI, MCP and language packages are
transports for engine operations. The Rust facade (`crates/dctx`) owns the
operations and their policy, selection, ordering, budgets and rendering. A new
surface must reuse those operations, not implement another answer pipeline.

Wrappers may start the engine, pass arguments through, stream results, map
errors and expose host-language idioms. They may never filter, sort, truncate,
retry with changed arguments, cache, parse pack text, count tokens, walk files
or read cache files. Chunk boundaries, cursors, selection decisions and
continuations belong to the engine. A wrapper forwards engine-produced chunks
without changing their bytes or inventing its own pagination.

Language packages use [the versioned stdio RPC protocol](protocol/README.md).
They negotiate its major version through hello and ignore unknown fields. They
can decode the protocol envelope to correlate IDs and map errors; operation
output stays opaque. A host idiom such as an iterator must only stream what the
engine produced. It cannot fetch, combine or reorder pages on its own authority.

CI enforces this boundary through the existing workspace gates:

- `crates/dctx-cli/tests/rpc.rs` compares exact bytes across RPC, CLI `--json`
  and corresponding advertised MCP tools on a committed fixture. It checks all
  protocol operations and preserves no-result output and CLI error codes.
- The same suite regenerates the schema from the facade Rust types and rejects
  a committed schema that differs by even one byte.
- RPC unit tests enforce ordered execution, malformed/oversized-line recovery,
  forward-compatible fields and the documented shutdown/cancellation contract.
- The RPC boundary source test rejects filesystem, cache, selection, budget and
  page-processing dependencies in the transport. Context selection, term
  extraction, token budgeting and rendering live in `crates/dctx/src/context`;
  CLI and RPC call the same facade method.

The workspace format, clippy and test jobs run these checks. Future package
cards must add their transport to the same corpus parity contract and provide
wrapper-boundary checks before the package ships. Tests enforce the current
surfaces; code review must also reject new wrapper logic that bypasses the
contract.
