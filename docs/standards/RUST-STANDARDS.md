# Rust standards for the host project (decision 073)

Binding for every Rust packet. Workers read this before writing code.

## Shape: domain-driven, small, readable

- **One crate per bounded context** (trace, store, gate, budget, scheduler/runtime, workers, key manager, shell…). A crate owns its domain types and rules; it never reaches into another crate's internals, only its public API.
- **Inside a crate:** `domain/` (types and pure rules, no IO) · `ports/` (traits for IO the domain needs) · `adapters/` (SQLite, process, network implementations) · `lib.rs` (public API only). Small crates may flatten this, but the domain stays IO-free.
- **Files stay small:** about 300 lines, one concept each. Name files and types after the domain words in the specs (`Packet`, `Attempt`, `Reservation`, `Halt`, `Grant`), not technical ones.
- **Header doc on every file** (3–6 lines `//!`): what it is, which spec section or TLA+ model it implements, what it depends on and who uses it. This is for low-context worker models and costs about 100 tokens to read instead of thousands to explore. No other commentary except where a rule is not obvious from the code.
- **Several binaries are fine:** a service, a CLI and tools each get their own `bin` crate that only wires library crates.

## Safety and correctness

- `#![forbid(unsafe_code)]` in every crate. `#![deny(missing_docs)]` for public items.
- No `unwrap`/`expect`/`panic!` outside tests. Errors are typed enums per crate (`thiserror`), propagated with `?`.
- **Make illegal states unrepresentable:** newtypes for ids, enums for states, state transitions as functions that consume the old state. Each state machine mirrors its TLA+ model; transitions not in the model don't compile or return a typed error.
- **The spec wins** over the Node reference (decision 072). Differences are recorded in `docs/design/SPEC-VS-NODE.md`.
- Determinism: inject clock, randomness and IO through ports, so tests and the DST harness replay exactly.

## Performance

- Borrow, don't clone; `&str`/`&[T]` in APIs; no allocation in hot loops. Bounded collections and bounded outputs everywhere (agents read the outputs).
- SQLite access batched in transactions; prepared statements cached.
- Async (tokio) only in adapters and binaries; domain code is synchronous and pure.
- Measure before optimising: `criterion` benches for hot paths named in the spec.

## Tests

- **Unit tests at the bottom of each file** in `#[cfg(test)] mod tests { … }`, covering that file's rules.
- **Property tests** (`proptest`) for every state machine and every invariant the TLA+ model states.
- **Integration tests** in `crates/<crate>/tests/` per public API; golden-corpus parity tests where the spec agrees with Node.
- Every refusal path has a test with a positive control next to it.

## The bar a packet must meet

`cargo test -p <crate> --locked` and `cargo clippy -p <crate> --locked -- -D warnings` on the dev machine, `cargo fmt --check`, no new `unsafe`, header docs present, and the TLA+ model it implements still checks. The continuous CI on the dev machine (decision 073) runs all of this for the whole workspace on every commit.

## Dependencies (approved list; L1 keeps it)

- **Workspace dependencies** (use `name.workspace = true`): `serde`, `serde_json`, `thiserror`, `sha2`, `hex`, `rusqlite`, `clap`, `regex`. Dev: `proptest`, `tempfile`.
- **Crate-local exact pins** (declared in that crate's own `Cargo.toml`, never in the root):
  - `cedar-policy = "=4.13.0"` in `the host tooling` only. It is the same engine as Node's `@cedar-policy/cedar-wasm` 4.13.0 (POLICY-BUNDLE.md §Evaluator).
- **Anything else:** stop and ask with one QUESTION line. Give the crate, its exact version, licence, whether it uses `unsafe`, and why the approved list cannot do the job. L1 answers within one watch cycle and adds approved crates here.
- Never edit the root `Cargo.toml`. Cargo.lock is refreshed by the VM CI; packets run without `--locked`.

### dctx workspace additions (decision 075, spec §13a)

- Also approved here: everything `dctx-core` already uses; `rusqlite` with `bundled` (FTS5) in `dctx-map-store`; `tree-sitter`, `tree-sitter-rust` and `tree-sitter-md` (exact pins) in `dctx-map-parse`.
- In dctx, workspace-level dependencies may be declared in the crate's own `Cargo.toml` with an exact version. The root stays untouched.
- Dev-dependencies only, for differential testing of first-party cryptography (decision 078, decision 081): `sha2` (against dctx-hash) and `ed25519-dalek` (against dctx-ed25519). They never appear in a production dependency closure.
- `syn` (exact pin, minimal features) in `dctx-lex-rust` only: the Rust structural extractor (decision 077).
