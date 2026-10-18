# Release inventory generators

Python 3 standard library generators for CONTRACT.md sections 5–7. The pure
functions return fresh dictionaries. Their CLIs encode ASCII-escaped UTF-8,
sorted keys, compact separators and one LF. Set-like arrays sort by identity.
Neither generator invokes Cargo, Git, networking, signing or publication.

## SBOM

Shell invocation (write access to the output must be granted by the caller):

```sh
python3 -B tools/plugin-release/sbom.py --metadata metadata.json --root dctx-cli --target aarch64-apple-darwin --epoch 1234567890 --out sbom.cdx.json
```

`cargo_metadata_to_cyclonedx(metadata, root_package, target)` selects normal and
build dependency edges of the requested target, excluding dev-only edges. It
supports exact triples and nested all/any/not target cfg expressions. Unknown cfg
atoms produce an unresolved result: dependencies are included conservatively,
with `cdx:cargo:cfg-unresolved` on the affected component (sorted cfg texts,
newline separated when several apply) and `cdx:cargo:cfg-unresolved-count`
on the document counting affected Cargo packages. Malformed expressions refuse.
Normal release target facts include atomic widths, unwind panic and disabled
test/debug assertions; optional inventory `target_features` lists enabled target
features (empty by default). Root is an unambiguous name or exact ID and must identify the
CLI; the standalone plugin contributes an independent dependency closure.

Cargo metadata cannot supply completed artifact hashes, native inventory,
source-closure hashes, source revision labels, or registry checksums. Enrich the
metadata dictionary with `metadata.metadata.dctx_release`, an explicit object:

- `target`, `profile` (default/strict), `source_revision` (full supplied hex label).
- `standalone_package`: unambiguous plugin name or exact Cargo ID.
- `artifacts`: keys `cli` and `standalone-git-plugin`, completed unsigned SHA-256.
- `source_hashes`: exact Cargo ID to SHA-256, required for non-registry crates.
- `license_files`: optional Cargo ID to reviewed `{expression, sha256}` objects;
  expressions must start with LicenseRef-. Include reviewed files as provenance
  materials too. No licence file is read or silently approved by the generator.
- `native_libraries`: explicit nonempty list of objects with fields `ref`, `name`,
  `version`, `purl`, `sha256`, `license`, `linkage`, `depends_on`, `used_by`.
  Linkage is static/dynamic/system. Both edge lists contain stable component refs.

The four-package fixture demonstrates this shape with synthetic inputs. It is
never real release evidence. Add `checksum` to registry packages from matching
name/version/source Cargo.lock entries. Missing registry checksums refuse. These
archive checksums do not independently establish reviewed extracted source hashes.
Missing/UNKNOWN/NOASSERTION licences, unresolved licence files and incomplete
inventories refuse. Caller approval and completeness cannot be proven by metadata.

Workspace members use first-party bom-refs `urn:dctx:first-party:NAME@VERSION`;
their required Cargo purls contain no source path. The frozen contract forbids a
scope field and extra first-party properties. Scope is expressed by graph selection.
CLI, standalone and bundled worker have distinct refs; the worker hashes its
containing CLI. Strict omits the worker. No timestamps or host paths are emitted.
`--epoch` validates the supplied epoch but cannot add a profile-forbidden timestamp.
Cargo resolve features must correspond to the exact intended profile/target;
the converter cannot reverse Cargo feature unification across targets.

## Provenance

`build_provenance` accepts keyword inputs: `subjects` (name to SHA-256), `builder_id`,
`toolchain`, `target`, `profile`, `source_revision`, `source_date_epoch`,
`build_inputs_bytes`, `cargo_lock_sha256`, `source_sha256`, `native_inputs_sha256`,
and optional `materials` (objects with uri and digest.sha256). All digests and
revision labels are supplied; the pure function performs no IO.
`test_only=True` checks statement shape through `check_test_shape` and accepts a
nonempty revision label without claiming production source evidence. The CLI
always uses full validation.

The CLI requires five repeated `--subject NAME=SHA256` arguments: CLI, standalone
plugin, plugin.json, SHA256SUMS and sbom.cdx.json. Other required arguments are
`--builder-id`, `--target`, `--profile`, `--source-revision`, `--source-sha256`,
`--native-inputs-sha256`, `--cargo-lock`, `--epoch`, and `--out`. Optional
`--build-inputs` defaults to the frozen recipe; `--material URI=SHA256` repeats.
The CLI hashes supplied lockfile bytes without invoking a build system.

The contract requires exact recipe bytes at material `urn:dctx:build-inputs`.
The requested canonical JSON digest is additionally recorded at
`urn:dctx:build-inputs:canonical` (canonical encoding includes LF). This preserves
both digest meanings while satisfying the frozen validator. Whitespace changes
to the approved recipe refuse. Provenance excludes itself and the envelope from
subjects; no SLSA assurance, approval or native qualification is inferred.

## Checks and real workspace runner

The kernel's ordinary unittest discovery uses no Cargo or network. Exact-output,
byte determinism, graph selection, licence/checksum refusals and closed fields
are checked with functions imported by path from validate_contract.py.

Shell runner for the real workspace, executed only by the kernel:

```sh
DCTX_RELEASE_REAL=1 python3 -I -B -m unittest discover -s tools/plugin-release -p test_sbom.py
```

The opt-in `test_real_workspace` is the metadata runner: it backs up Cargo.lock
bytes in memory, invokes `cargo metadata --format-version 1 --offline`, and retries
once without `--offline` if the first command exits nonzero. A final failure shows
the exit code and last 20 stderr lines. In a finally block it restores the original
bytes only if the lockfile still contains the bytes observed after Cargo; a
concurrent edit is preserved. It merges registry checksums from Cargo's resulting
lockfile snapshot, which matches the metadata even if resolution updated the lock,
and generates the SBOM and provenance in memory.
Python 3.11+ is required for this adapter's stdlib TOML parsing. Run with
exclusive access to the lockfile. No additional environment is required.

Default test inputs are explicitly synthetic: revision `unversioned`, native
libraries `[]` with note `none declared in test run`, CLI stand-in = SHA-256 of
Cargo.lock, plugin/path-source stand-in = SHA-256 of CONTRACT.md (hashlib).
Provenance subjects are Cargo.lock, CONTRACT.md, plugin.json, SHA256SUMS and
sbom.cdx.json. The two middle metadata names reuse the artifact stand-in digests;
the SBOM subject hashes the actual generated JSON. Source material hashes the
contract stand-in; native material hashes the canonical libraries/note object.
Epoch 0 and the recipe's toolchain are test inputs, not observed build evidence.

Optional overrides: `DCTX_RELEASE_INVENTORY` is a JSON object with any inventory
fields described above; missing fields receive defaults. `DCTX_RELEASE_REVISION`,
`DCTX_RELEASE_CLI_SHA256`, and `DCTX_RELEASE_PLUGIN_SHA256` override their respective
defaults when that field is absent from the inventory object. No Git is invoked.
The runner always sets `test_run: true`; missing native libraries require the
explicit test note. Production conversion still requires a nonempty inventory.

The real run checks the closed SBOM schema from sbom-profile.json, unique
component/dependency refs, one dependency row per component, no dangling edges,
and byte-identical repeated conversion. It calls validate_contract.py's structural
`validate` function on every provenance field except the source_revision evidence
constraint; that field must instead be a nonempty supplied label. Closed object
keys, required fields, subject count/names/digest syntax, target/profile enums,
toolchain constant, builder syntax, material structure and epoch bounds are checked.
It also checks repeated provenance bytes, exact subject digests (including the
generated SBOM), SBOM/provenance target/profile/revision equality, the lock digest
and the exact approved recipe digest. Production semantic `check_sbom` and
`check_provenance` are deliberately not used for the real smoke test: source/native
evidence and exact release feature closure are absent. These are non-promotable
test documents, not a frozen-contract test trust profile; default documents fail
the full frozen checks. No schema or validator file is changed.

With DCTX_RELEASE_REAL absent, the adapter skips. The worker runs only the offline
packet unit tests; the kernel alone executes the real Cargo predicate.
