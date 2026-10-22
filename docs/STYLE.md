# Rust house style

Keep the surrounding source layout, including wide lines when they keep a declaration, match arm, protocol record, or assertion readable as one unit. Prefer small, focused edits over unrelated layout changes. Preserve module headers, explicit boundary types, and behavior; use readable numeric separators and document fallible public operations.

## Formatting and the worker boundary

Rustfmt is not adopted. Its default reflow expands compact source into many more lines and would break the existing plugin-git file-length boundary test. `plugins/dctx-plugin-git/tests/boundary.rs::worker_sources_have_no_direct_effects_and_carry_headers` embeds 23 worker source files and requires each to have at most 300 lines, alongside checks for module headers, unsafe-code policy, and forbidden direct effects. Keep that boundary intact when adding documentation or lint attributes. Split a module deliberately if needed; do not weaken the test to accommodate a formatter.

Do not add `cargo fmt --check` to CI. Formatting is reviewed in context.

## Pedantic style gate

Run the style gate on the remote build host or the dctx CI runner:

```sh
cargo clippy --workspace --all-targets -- -W clippy::pedantic
cargo doc --no-deps
```

Resolve each diagnostic with a genuine improvement or a narrowly scoped attribute of the form `#[allow(clippy::lint_name, reason = "specific justification")]`. Put the allow on the affected function, type, implementation, or test module; avoid blanket crate-level suppression. Keep public API signatures and behavior stable. Keep rustdoc warning-free, and retain the existing CI clippy and test checks before pushing a batch.

Documented exceptions:

- `many_single_char_names` on SHA compression functions: the a-h working variables follow FIPS 180-4 and make comparison with the specification straightforward.
- `must_use_candidate` and `return_self_not_must_use` on existing public constructors, accessors and value operations: preserve the API's existing caller lint behavior while keeping signatures and runtime behavior stable. Existing `#[must_use]` contracts remain in force.
- `struct_field_names` on the Rust extractor's limits: `max` distinguishes ceilings from observed byte, nesting and record counts.
- `cast_precision_loss` on the Rust differential test's scoring helper: counts from the fixed snippet corpus are far below the exact integer range of `f64`.

The workspace-wide pedantic cleanup is being landed in crate-sized batches. The command above is the house style gate; the current CI workflow still runs its existing `-D warnings` clippy command and tests. The full workspace includes the Tauri desktop app and needs its platform development libraries. A run with `--exclude dctx-desktop` is useful for the other crates but does not establish full-workspace acceptance.
