# Independent review: legacy IO removal

| Unit | Verdict | Result |
|---|---|---|
| DX-D3-02: legacy_codec, legacy_decode, legacy_encode | PASS | No finding in the bounded byte codec itself; imported-version use is covered separately below. |
| DX-D3-02: facade import_op | FAIL | F6: accepted history can make the next pack overflow. |
| DX-D3-03b: legacy_output, legacy_diff, facade pack_op | FAIL | F5: output bounds are checked after potentially enormous allocation. |
| DX-D3-04c: compat, main alias route, legacy_alias tests and called host operations | FAIL | F1, F3, F4, F7. |
| DX-D3-06: installer and install-no-git oracle | FAIL | F8: reinstall overwrites user-edited templates. No finding in the removed Git/ignore-edit path itself. |
| DX-D3-06: architecture-bench | FAIL | F2, F9, F11. |
| DX-D3-08b: core purity and retained core standards | FAIL | F10, F11. No current production IO call found in the core source audit. |
| DX-D3-08b: migrated recall rows | PASS | No finding in the replacement symbol mappings. |

Static review only, 2026-10-18. Source was read in bounded ranges; no Cargo, installer, benchmark, test suite or reproducer was executed. PASS means no static finding within the stated boundary, not a test result. Only this report was written. Findings in retained helpers are identified as such; no claim is made that every defect was introduced by these packets.

## Findings, ordered by severity

### F1 — P1: legacy writes can escape their intended destinations

**Locations:** `crates/dctx-cli/src/compat.rs:74`, `:136`, `:181`; `crates/dctx-cli/src/lib.rs:87`, `:199`.

The registry returns arbitrary IDs, which are joined directly to the cache path. Both binary saves and text exports use truncating `fs::write`, following existing links; neither the destination nor its parents are confined. Running the alias on a repository can therefore overwrite an unrelated file through a repository-supplied export symlink, without an explicit output argument.

**Minimal reproducers (isolated fixtures):**

- In a project named `Corpus`, create `.dctx/Corpus.llmcontext.txt` as a symlink to a sibling sentinel file; add an included `a.txt`; invoke the alias on that project. The sentinel is replaced by the snapshot. A symlink for the entire `.dctx` directory also redirects exports.
- Give `projects.db` a row `/tmp/review-victim | <canonical project path>` and precreate `/tmp/review-victim.dctx` with sentinel bytes. The absolute ID discards the cache prefix and the alias replaces that file. A `../review-victim` ID similarly traverses upward.

**Fix:** Validate historical IDs as nonempty single path components and reject absolute/traversal IDs. Confine and validate parent directories and use a safe atomic replacement policy that does not follow destination links; test sentinels outside both roots alongside normal successful writes.

### F2 — P1: benchmark deletes unowned directories, including paths selected by manifest IDs

**Locations:** `tools/architecture-bench/src/lib.rs:145`; `tools/architecture-bench/src/measure.rs:101`, `:124`, `:237`, `:246`; `tools/architecture-bench/src/manifest.rs:88`.

The retained runner recursively deletes `--scratch`, the fixture subdirectory, and an existing corpus cache. The only cache check excludes the source itself and its descendants; it neither proves ownership nor rejects source ancestors. Manifest corpus IDs need only be nonempty, so an absolute ID or `..` can redirect `root.join(id)` outside the supplied cache root.

**Minimal reproducer:** Use a valid manifest with `dctx.id = "/tmp/review-victim"`, where that directory contains a sentinel and is outside the source. Run the benchmark's `--only dctx` mode with an unrelated disposable `--cache`; after hashing, it removes `/tmp/review-victim`. Independently, parent mode with `--scratch /tmp/review-victim` deletes it before spawning either child. Existing tests at `measure.rs:259` exercise only the lexical inside/outside predicate.

**Fix:** Restrict corpus IDs to single components and allocate owned per-run directories, refusing preexisting unowned paths. Before any cleanup, verify ownership and canonical containment and reject any source ancestor as well as source descendants.

### F3 — P1: local omit rules hide output but retain the omitted source in the binary cache

**Locations:** `crates/dctx-cli/src/compat.rs:152`; `crates/dctx-cli/src/lib.rs:137`; `crates/dctx-cli/tests/legacy_alias.rs:194`, `:208`.

`scan_policy` deliberately passes `local_omit: None`. The alias reads those files and saves their original bodies before applying the local omit rules to rendering. This is explicitly preserved by D3-COMPATIBILITY, but conflicts with ADR-014's OMIT “Read File into Memory? NEVER” contract; the migration's own test asserts the retained body rather than a no-read guarantee. It must not be presented as satisfying the general omitted-source privacy rule.

**Minimal reproducer:** Put `private.txt\n` in `.dctxomitcontent` and `UNIQUE_PRIVATE_SENTINEL` in `private.txt`, then run `dctx .`. Text shows an omission marker, but decoding the registry-selected cache file with `LegacyBodies::Include` returns the sentinel. The existing `kept.rs` test already demonstrates this behavior.

**Fix:** Apply local omit rules during scan authorization, before content reads and state encoding, and amend the conflicting compatibility expectation. Add a denied-read control for the locally omitted file beside an included-file read control, and assert the sentinel is absent from saved state.

### F4 — P1: template failure truncates the previous export and still reports success

**Locations:** `crates/dctx-cli/src/lib.rs:149`, `:163`, `:199`; `crates/dctx-cli/src/template.rs:152`; `crates/dctx-cli/src/compat.rs:181`.

A missing or invalid configured template only prints a generation error. `body` stays empty, then the alias unconditionally writes it over the normal export and returns `Ok(())`. Binary state has already been saved, so the failed operation can also consume a version change.

**Minimal reproducer:** Produce a valid export with the default template, retain its bytes, then set `default_template = 'missing'` in the isolated config without creating `cache/dctx/templates/missing.bin`. Run the alias again with at least one included file. The expected preservation/error assertion fails: the previous text becomes zero bytes and the command succeeds.

**Fix:** Propagate template/render errors and produce all required documents before replacing existing state or exports. Add missing-template and malformed-template cases asserting nonzero exit and unchanged files, with a valid-template control.

### F5 — P1: the facade's byte cap does not bound legacy rendering allocation

**Locations:** `crates/dctx/src/session/pack_op.rs:71`, `:73`; `crates/dctx-template/src/legacy_output.rs:54`; `crates/dctx-template/src/v1/render.rs:144`, `:146`.

The facade constructs the entire rendered `Vec<u8>` before checking `encoded_bytes`. Legacy rendering repeatedly expands caller-supplied template strings with unbounded `String::replace`, without a fallible size calculation or a rendering limit. The small pinned one-byte-short tests check sink atomicity, not allocation safety.

**Concrete input:** One included 8 MiB text file, a template whose file partial is `"{{{file.content}}}".repeat(4096)`, a layout containing `{{{comp.files}}}`, and `encoded_bytes = Some(1024)`. The partial alone expands to 32 GiB before the 1 KiB refusal can run, potentially aborting the host. This fits the default per-file scan ceiling.

**Fix:** Pass effective limits into the legacy renderer and preflight checked expansion sizes or render through a bounded fallible buffer before allocating replacements. Add an expansion-bomb test with an allocation/work bound and an exact-fit successful control.

### F6 — P2: accepted maximum revisions overflow when imported history is used

**Locations:** `crates/dctx/src/session/import_op.rs:52`; `crates/dctx-pack/src/legacy_codec.rs:127`; `crates/dctx-core/src/version.rs:78`; `crates/dctx-pack/tests/legacy_import.rs:61`.

The codec intentionally accepts any positive major, including `u32::MAX`, and import publishes it unchanged. The retained version transition uses `self.major + 1`; a subsequent major reset panics with overflow checks and wraps to invalid major zero without them. The alias reaches the same transition after loading such a state. The codec's maximum-version positive case does not exercise a subsequent pack.

**Minimal reproducer:** Copy the valid `BYTES` document from the facade import test and replace bytes `8..12` with `u32::MAX.to_le_bytes()`. Import it into the next pack's view, then pack a source with one changed file and no unchanged files. The direct arithmetic reproducer is the decoded version's `calculate_next(0, 1, 999)`.

**Fix:** Make version exhaustion a checked, typed refusal and propagate it before publication in both consumers. Preserve decoding of historical revisions, but add a maximum-revision import-then-pack test and a `u32::MAX - 1` successful transition control.

### F7 — P2: alias removes scan resource bounds before the bounded codec is reached

**Locations:** `crates/dctx-cli/src/lib.rs:106`; `crates/dctx/src/adapters/fs.rs:84`, `:93`; `crates/dctx-cli/src/compat.rs:135`.

The alias chooses `PackLimits::unbounded()`. Consequently, the filesystem source reads an included file in full and the scanner retains all included bodies; the codec's 64 MiB limit is applied only afterward. A hostile regular file can exhaust memory even in tree-only mode, and an otherwise manageable oversize snapshot only logs a save failure while continuing to emit output.

**Minimal reproducer / missing test:** Give an isolated project a sparse multi-gigabyte `large.txt` and invoke `dctx . --tree`; no finite file/source limit stops `std::fs::read`. Add a much smaller injected-limit test that proves oversized content is refused or withheld before the read, with a normal file as a positive control.

**Fix:** Use explicit finite scan limits consistent with the alias's state format, including entry and depth bounds, and propagate state-save refusal. Apply any tree-only no-content optimization only after reconciling its state compatibility contract.

### F8 — P2: reinstall silently destroys edited templates

**Locations:** `tools/install.sh:91`; `tools/tests/install-no-git.sh:63`, `:72`.

The installer protects an existing config but unconditionally copies every shipped template over the user's template directory. The second-install test checks only config and Git-ignore preservation; its templates are unchanged copies of the distribution, so it cannot detect loss of edits.

**Minimal reproducer:** After a successful isolated installation, replace `$XDG_CONFIG_HOME/dctx/templates/default.dctx_template` with sentinel/custom-template bytes and reinstall. The unconditional `cp -r` replaces them. The same issue affects staged user edits in `ignore.new` and `omitcontent.new` when those update branches run.

**Fix:** Preserve existing modified assets and stage updates under a non-clobbering name, or require an explicit replacement option. Extend the existing second-install test with customized template and staged-update sentinels while retaining the fresh-install assertions.

### F9 — P2: benchmark still uses external SHA-256 in production

**Locations:** `tools/architecture-bench/Cargo.toml:17`; `tools/architecture-bench/src/pin.rs:7`.

`sha2` is a normal dependency and supplies both per-file and aggregate hashes at runtime. Removing the Git provenance command did not remove this retained decision 078 violation; the binding Rust standard explicitly allows `sha2` only as a differential-test dev-dependency.

**Exact missing check:** Assert that the benchmark's normal dependency graph excludes `sha2` and that its pin implementation uses `dctx_hash::Sha256`. Retain fixed known hash vectors and compare identical corpus pins across the change.

**Fix:** Replace the production hashing adapter with the first-party crate, propagating its typed failures. Keep any independent `sha2` comparison in dev-dependencies only.

### F10 — P2: the purity oracle skips reachable production code

**Location:** `crates/dctx-core/tests/purity.rs:28`.

`violations` stops at the first line beginning `#[cfg(test)]`, rather than excluding just that annotated item. Its positive controls cover simple forbidden tokens, but not reachable production code after a test module or ordinary grouped/aliased imports. Thus “every source module's production code” is not enforced.

**Minimal missing controls:** Both valid Rust snippets below currently return an empty violation list:

- `#[cfg(test)]\nmod tests {}\npub fn leak() { let _ = std::fs::read("x"); }`
- `use std::{fs as disk};\npub fn leak() { let _ = disk::read("x"); }`

**Fix:** Scan production items with scope-aware token handling rather than truncating the file, and resolve or reject forbidden module aliases/grouped imports. Add both refusal controls beside a truly test-only IO block that remains allowed.

### F11 — P3: retained files exceed the size standard and core/CLI omit the required documentation lint

**Locations:** `tools/architecture-bench/src/report.rs:1` (363 lines); `crates/dctx-core/src/diff.rs:1` (540); `crates/dctx-core/src/file_classifier.rs:1` (514); `crates/dctx-core/src/filter.rs:1` (424); `crates/dctx-core/src/version.rs:1` (368); `crates/dctx-core/src/lib.rs:6`; `crates/dctx-cli/src/lib.rs:6`; `crates/dctx-cli/src/main.rs:4`.

These retained files violate the requested roughly-300-line shape check. Core and CLI crate roots also lack `#![deny(missing_docs)]`, explicitly required by RUST-STANDARDS; core still exposes undocumented public items, for example `diff.rs:10`. This is a standards finding, not a claim that legacy IO deletion introduced the oversized files.

**Exact missing check:** A source-size check for the listed files and a crate-root check requiring the documentation lint would fail. No missing `//!` header was found in the named reviewed Rust units.

**Fix:** Split the oversized modules by domain responsibility while preserving coverage and public APIs. Document the exposed items and enable the required lint in the core and CLI roots.

## Compatibility and scope qualifications

- The inspected D3-01 snapshot/tree/diff tests compare implementation output to fixed fixtures, including headers, SVG sentences, IDs and closing-tag placement. Codec tests compare explicit wire bytes and exercise truncation, depth, offsets and exact bounds; no vacuous byte-parity finding.
- The two ignored selection tests remain at `crates/dctx-core/tests/selection_legacy.rs:250` and `:260` for `.turbo/` and `.pnpm-store/`. D3-COMPATIBILITY explicitly records these preexisting differences; they are not passing parity evidence, and the inspected pinned corpus does not cover them.
- The migrated recall rows name live replacements and retain definition-existence assertions in `crates/dctx-map-query/tests/repository.rs:79`. The recall score itself is asserted only when `DCTX_MAP_RECALL` is set; symbol existence in the default suite does not establish recall quality.
- No runtime Git invocation was found in the reviewed alias route, installer, codec/render/import paths or benchmark provenance. Clipboard execution is explicitly requested; benchmark self-spawn, `rustc` and `ps` are not Git.
- Reviewed crate manifests declare Apache-2.0, and the reviewed crate roots forbid unsafe. No first-party unsafe implementation was found. Apart from the retained production `sha2` issue, no external dependency is required by the new codec/render/import code; dependency additions over time were not inferred without a historical diff.
- Reset/corruption paths do not explicitly sweep legacy text exports. That narrow deletion property does not prevent the overwrites in F1/F4 or the benchmark deletion in F2. No source-content-in-error/log finding was established in the reviewed paths.
