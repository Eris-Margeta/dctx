# Code guidelines for the harness and every place (decision 073, decision 074)

Binding for the oracle, place agents and workers, in every language. Rust specifics: `RUST-STANDARDS.md`.

## Written for the hierarchy that reads it

Code here is read far more often than written, mostly by models with small context windows and low reasoning effort:

- the oracle verifies designs;
- place agents plan packets;
- workers change one packet's files.

Everything below serves that reader.

1. **Domain-driven design.**
   - One module (crate, package) per bounded context, named in the domain words of its spec.
   - The domain is pure (no IO); IO sits behind ports and adapters.
   - A module touches another only through its public API.
2. **Small files, one concept each,** about 300 lines. A worker should understand a file from the file alone.
3. **A header doc on every file** (3–6 lines):
   - what it is;
   - which spec section or formal model it implements;
   - what it depends on;
   - what uses it.

   It costs about 100 tokens to read and saves thousands of tokens of exploration. Other comments only where a rule is not obvious from the code; never narrate the code.
4. **Stable names and ids.** Types, states and errors use the spec's words, so the oracle can check code against the spec line by line.
5. **Errors are typed and explicit.** No silent fallbacks. Unknown means refuse.

## Tests

- **Unit tests** sit next to the code they test, at the bottom of the file (Rust `#[cfg(test)] mod tests`; elsewhere the language's closest equivalent).
- **Property tests** for every invariant; integration tests per public API.
- **Every refusal path** has a test with a positive control.
- **Continuous CI** on the dev machine runs every test, the linter and the formal checks on every commit (decision 073).

## Critical projects are mathematically described (LAWS rule 45)

A place labelled critical (LAWS rule 41) has a mathematical description of its core before or alongside its code:

- **A formal model** (TLA+ or equivalent) of every state machine and protocol, with the safety and liveness properties it must keep, checked by the model checker in CI. Mutation configs prove the properties are not vacuous.
- **Invariants written down** as numbered properties, each with a property test in the code.
- **Quantities that drive decisions** (budgets, pace, criticality, the resource objective) are written as formulas with their inputs and units (docs/specs/GATED-PARAMETERS.md, docs/proposals/RESOURCE-OBJECTIVE.md).
- **The spec wins** over any existing implementation (decision 072); every difference is recorded.

The oracle verifies every design point of a critical project and refuses code whose behaviour has no model or property behind it.
