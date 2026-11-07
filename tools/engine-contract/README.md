<!-- SPDX-License-Identifier: Apache-2.0 -->
# Engine contract CI guards

These dependency-free Python 3.11+ scripts enforce
[the engine boundary](../../docs/PHILOSOPHY.md). Run from the repository root:

```sh
python3 tools/engine-contract/check_imports.py
python3 -m unittest discover -s tools/engine-contract -p 'test_*.py' -v
cargo build --locked -p dctx-cli
python3 tools/engine-contract/parity.py target/debug/dctx-cli
python3 tools/engine-contract/warm_read.py target/debug/dctx-cli
```

`wrapper-api.toml` is the reviewed import allowlist. MCP and RPC share the CLI
crate's manifest, but their source directories may only reference the listed
application entry points and wire types. Grouped imports, aliases, reexports,
fully qualified paths and Cargo dependency renames are checked. Binding and npm
package trees are discovered recursively, including future package additions;
bindings may not link engine crates. New standalone transport crates matching
`dctx-mcp*` or `dctx-rpc*` are covered too. Add other wrapper locations to the
policy before shipping them. This conservative source lint supplements Rust
visibility and review; it does not prove the semantics of arbitrary wrapper code.

`warm_read.py` primes a real persistent index over a generated project with 32
unrelated source branches, then repeats an explicit one-file line read under
Linux `strace`. The expected cache-open set is **empty**: scoped reads use an
ephemeral facade session. Even attempted opens of ledger, locks, policy or
objects fail the check. The trace must show the selected source being opened,
at most three times (line-range preparation, verification and emission), and
must show no unrelated source open/read/mmap or unrelated
directory enumeration. Engine-created cache objects and decoy files exist
before tracing so a missing cache cannot make the check pass. The warm page
must equal the preceding read byte for byte. Non-Linux hosts, missing `strace`,
and denied ptrace explicitly print `SKIP`; other tracing/engine failures fail.
CI installs `strace` using its existing trace-tool step.

`goldens/cases.json` is the transport-neutral corpus. Its checked-in `.golden`
files contain unmodified CLI engine stdout, including final newlines. The
harness compares those bytes independently against CLI, MCP and RPC, decoding
only their protocol envelopes. No JSON canonicalization, trimming or sorting
is permitted. The initial cases cover line reads and empty search results;
the existing Rust `tests/rpc.rs` suite covers the broader operation/error matrix.
To update a golden, capture the case's CLI stdout on the committed repository
fixture using isolated configuration/cache directories and review the exact
diff. The harness never updates goldens automatically.

New surfaces can share the corpus without changing its assertions:

```sh
python3 tools/engine-contract/parity.py target/debug/dctx-cli \
  --adapter package=path/to/package_parity_adapter.py
```

An adapter exports `output(context, case) -> bytes`. `context` supplies the
absolute engine binary, fixture directory, isolated environment and a bounded
subprocess runner. The case supplies CLI arguments and RPC/MCP requests. Start
the actual package with this engine and environment, decode its envelope and
return the untouched operation bytes. Register package adapters in CI as they
ship; every registered adapter must pass every case. Duplicate surface names
and non-byte outputs fail.
