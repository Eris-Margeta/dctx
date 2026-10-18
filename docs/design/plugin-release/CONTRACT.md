# Release formats v1 — PL-RELC

Normative PA decisions under decision 114, 2026-10-18; reversible by a versioned contract change.
Implements REL-C in `PL-REL-PLAN.md:120` and its §§2–4. Depends on the existing distribution contracts below.
REL-T/B/M/I/G/K consume these six files. This packet defines formats and an offline structural validator;
it neither implements product verification nor establishes a reproducible build, native qualification, trust or publication permission.

## 1. Existing formats remain authoritative

Paths below are repository-relative; citations refer to this checkout. No plugin/index schema is duplicated here.
The packet's named `crates/dctx-plugin-distribution/README.md` is absent; source and frozen schemas are the authority.

| Existing object | Closed fields / rules | Source file:line |
|---|---|---|
| Manifest | Required `manifest_version=1,id,name,version,release_sequence,publisher_key,license,protocol,host_api,hooks,commands,capabilities,config,limits,reason_codes,diagnostic_codes,artifacts`; optional `state_version` | `crates/dctx-plugin-distribution/src/domain/manifest.rs:20`, `:79` |
| Artifact | Required `target,file,bytes,sha256,format="native-executable",min_os,runtime_resources`; optional `libc` | `crates/dctx-plugin-distribution/src/domain/manifest.rs:25`, `:143` |
| Runtime resource / libc | Resource `{file,bytes,sha256}`; libc `{name:"glibc",min_version}`. Positive sizes, lowercase SHA-256. Linux release generation requires a measured libc floor even though parsing makes it optional. | `crates/dctx-plugin-distribution/src/domain/manifest.rs:26`, `:132`, `:154` |
| Nested manifest | Protocol `{major,min_minor,max_minor}`; host API `{major,min_minor}`; hooks `{name,version}`; commands `{alias,verbs,output,allow_text}`, output `{name,fields}`; capabilities `{required,optional,outputs,network:[]}`. Config, limits and codes use their existing referenced schemas/semantics. | `docs/design/plugin-api/manifest.schema.json:14` |
| Index | Top `{schema:1,sequence,plugins}`; plugin `{id,repository,summary,publisher_key_fingerprint,releases}`; release `{version,release_sequence,manifest_url,manifest_sha256}` | `crates/dctx-plugin-distribution/src/domain/index.rs:21`, `:78`, `:91`; `plugins/index/index.schema.json:15`, `:34` |
| Checksum mirror | `SHA256SUMS` lists exactly all executable/runtime payloads in the manifest, all targets; no CLI/SBOM/index extras. Output `<sha256>  <filename>\n`, sorted filenames; parser also accepts ` *`. Cap 65,536 bytes. | `crates/dctx-plugin-distribution/src/domain/sums.rs:12`, `:16` |
| Signatures | 64 raw Ed25519 bytes; manifest domain `dctx-plugin-manifest-v1`, index domain `dctx-plugin-index-v1`; message is ASCII domain directly followed by exact JSON bytes | `crates/dctx-plugin-distribution/src/domain/signature.rs:10`, `:18` |
| Offline container | `DCTXPLG1`, LE u32 table length, closed `{schema:1,entries:[{name,bytes,sha256}]}`, then payloads. Reserved names: `plugin.json,plugin.json.sig,SHA256SUMS,LICENSE,NOTICE,provenance.json`. | `docs/design/plugin-api/distribution.md:68`; `crates/dctx-plugin-distribution/src/domain/manifest.rs:18` |
| Package/profile inputs | CLI 0.25.0, default `git-worker,dctx/plugin-runtime`; strict uses `--no-default-features --features strict`. Standalone Git 1.0.0, binary `dctx-plugin-git`. | `crates/dctx-cli/Cargo.toml:3`, `:7`; `plugins/dctx-plugin-git/Cargo.toml:3`, `:12` |

The SBOM, release envelope and evidence stay outside `.dctx-plugin`; never smuggle them in as runtime resources.
Manifest signature and complete `SHA256SUMS` remain independently required. The new envelope hashes that same checksums file.
There is no second CLI checksum file in v1: the signed envelope carries the CLI SHA-256 directly.

## 2. New release envelope (`release.json`)

`release.schema.json` is draft 2020-12. Every object is closed; all fields below are required.
One envelope describes one target/profile, with independent CLI and plugin version axes.
All digests are lowercase 64-hex SHA-256 of exact completed bytes; positive sizes are integers ≤ 2^53−1, never booleans.
`artifact` means `{file,bytes,sha256}`. Files are safe single basenames, unique ignoring case, with no traversal/device names.

| Field | Meaning / constraints |
|---|---|
| `schema`, `kind`, `version` | `1`, `"dctx-release"`, CLI semantic version |
| `plugin` | `{id:"dctx/git",version,release_sequence}`; must match the separately verified manifest and selected index row |
| `target`, `profile` | Full Rust triple from §4; `default` or `strict` |
| `source_revision`, `build_inputs_sha256` | Full lowercase 40/64-hex revision label; digest of exact approved `build-inputs.json` bytes |
| `artifacts` | Exactly `cli,plugin,manifest,checksums,sbom,provenance`, each an `artifact`; plugin is the standalone native binary |
| Metadata filenames | `plugin.json`, `SHA256SUMS`, `sbom.cdx.json`, `provenance.json`; CLI/plugin filenames must match the actual packaged target files |
| `bundled_git_component` | `"dctx/git/bundled"` for default, `null` for strict; references a distinct SBOM component |
| `authentication` | `{algorithm:"Ed25519",domain:"dctx-plugin-index-v1",key_fingerprint,index_sha256,index_sequence,trust}`; trust is `test` or `production` |
| `qualification` | `{state,reason,native,target,profile,source_revision,build_inputs_sha256,cli_sha256,plugin_sha256,provider,records}` |
| Qualification values | State `candidate` or `promotable`; candidate needs a nonblank reason, promotable an empty reason. `native` is boolean. Binding fields must equal the enclosing release and completed CLI/plugin digests. |
| `provider` | `unqualified`, `qualified`, or `bundled-git-pending-confinement`; pending is default-only and has the limited decision 080 meaning below |
| `records` | Closed object with optional `native,reproducibility,crypto,lifecycle,privacy,git,image_binding,native_inputs`, each an `artifact`. All eight are mandatory for a promotable shape. |

An evidence record must independently bind the same target, profile/features, source, recipe, artifact digests,
OS/CPU/libc/SDK floors and provider. Its exact document bytes are authenticated by the envelope's digest.
The producing platform/crypto/lifecycle packet owns its evidence format; this contract does not invent approval receipts.
REL-G verifies record content, issuer, freshness and current acceptance. A filename, hash, boolean or `state` declaration alone is no proof.
Evidence records and their referenced inputs must be present for real promotion, including approved exact native tool pins.

## 3. Authentication linkage (new release-tool behavior)

`release.json.sig` is a separate 64-byte raw signature over
`ASCII("dctx-plugin-index-v1") || exact_bytes(release.json)` by the independently pinned index authority.
There is no NUL, newline, length prefix, prehash, JSON reserialization or embedded signature field.
Reuse the existing `verify_detached` primitive; a new release parser checks `kind="dctx-release"` and this closed schema.
The closed index schema and closed release schema are disjoint. Consumers and signing requests MUST specify the document kind;
never feed an envelope to `VerifiedIndex`, or accept an index as an envelope just because the shared-domain signature verifies.

Verify the ordinary whole-index signature separately using the same approved index key, then match its exact digest and sequence
to `authentication.index_sha256/index_sequence`. Match the selected plugin ID/version/sequence and `manifest_sha256`
to this envelope's plugin identity and manifest digest. Independently verify the publisher's manifest signature/domain,
pinned publisher fingerprint, selected target artifact digest/size, capabilities and checksum mirror.
Fingerprint means SHA-256 of the exact 32 public-key bytes, never a key ID (`docs/design/plugin-api/distribution.md:26`).

The existing index signature does **not** authenticate a sibling envelope: the new detached envelope signature is indispensable.
The index remains unchanged; it does not point back to the envelope. A sidecar locator is untrusted until these checks pass.
Do not replace `manifest_sha256` with an envelope digest or add fields to either frozen document.
Enforce pinned trust, revocation/freshness, monotonic index and plugin sequences, rotation and explicit rollback rules.
`trust` is a checked assertion, never a trust selector; production trust comes from the host/key manager-approved store.
No downloaded key, checked-in fixture key or `local-test-pins` file bootstraps trust (`tools/plugin-index/src/domain/signature.rs:26`).
REL-K must bind document kind, domain, exact message, digests, identity and sequences to its scoped signing request.
Signature fields are outside the hash graph; no document contains its own final digest. No private key or new signing grant exists here.

## 4. Targets and native profile gate

Existing triples come from `crates/dctx-plugin-distribution/src/domain/names.rs:161`, `:178`.

| Label | Triple | Promotion policy / present blocker |
|---|---|---|
| linux-x64 | `x86_64-unknown-linux-gnu` | Initially eligible; exact native runner/compiler/linker/glibc floors and accepted evidence still required |
| macos-arm64 | `aarch64-apple-darwin` | Initially eligible; exact native SDK/linker/OS floors and accepted evidence still required |
| macos-x64 | `x86_64-apple-darwin` | Blocked: outside initial set; no qualified native Intel Mac inputs/evidence |
| linux-arm64 | `aarch64-unknown-linux-gnu` | Blocked: outside initial set; no qualified native aarch64 inputs/evidence |
| windows-x64 | `x86_64-pc-windows-msvc` | Blocked: outside initial set; no qualified native MSVC/SDK inputs/evidence |
| every other target | including musl / unknown CPU or ABI | Blocked: outside the closed manifest set; refuse unknown triples, never fall back |

Both default and strict require their own native evidence; a successful cross-build, fake provider or another profile cannot substitute.
Invariant P1: promotable ⇒ eligible exact target ∧ native evidence ∧ all eight accepted evidence records ∧ production trust.
Invariant P2: every record binds this exact source/recipe/target/profile and completed artifact digests; mismatch or staleness refuses.
Invariant P3: strict contains no bundled worker. Default contains the separately inventoried worker with the CLI's containing digest.
decision 080 permits `bundled-git-pending-confinement` only for verified containing-distribution Git, with consent, lifecycle, privacy,
broker/Git and image-binding evidence. It claims no isolation. Standalone/installed overrides still require a qualified provider before enablement.
Packaging the standalone binary does not authorize its execution. General launcher support remains blocked as described in `PL-REL-PLAN.md:109`.
No target is proven promotable by this packet; eligibility is a policy choice, not native qualification.

## 5. Provenance and SBOM

`provenance.schema.json` closes an in-toto Statement v1 with SLSA provenance v1 predicate subset; no SLSA assurance level is claimed.

| Provenance path | Required content |
|---|---|
| `_type`, `predicateType` | `https://in-toto.io/Statement/v1`, `https://slsa.dev/provenance/v1` |
| `subject[]` | Exactly five unique `{name,digest:{sha256}}`: CLI, standalone plugin, manifest, checksums, SBOM; match envelope filenames/digests |
| `predicate.buildDefinition.buildType` | `urn:dctx:release:build:v1` |
| `…externalParameters` | `{target,profile,source_revision,toolchain}`; bindings equal envelope; toolchain exactly the approved object in `build-inputs.json` |
| `…internalParameters` | `{source_date_epoch}`: nonnegative integral source revision Unix seconds, supplied without runtime Git |
| `…resolvedDependencies[]` | Unique `{uri,digest:{sha256}}` materials; required URIs `urn:dctx:build-inputs`, `urn:dctx:source`, `urn:dctx:lock:workspace`, `urn:dctx:native-inputs` |
| `predicate.runDetails.builder.id` | Stable approved `urn:dctx:builder:<identity>`; same pinned builder identity across the two reproducibility runs |

Material digests cover exact recipe bytes, source archive/tree closure, lockfile and native input inventory respectively.
Add every participating tool-workspace lock, native tool/SDK, dependency source closure and reviewed licence file as unique materials.
Rust/native SBOM source components use their purl as material URI. A source revision is a label, not a replacement for the source SHA-256.
Provenance excludes itself, envelope, signatures and observations from subjects to avoid cycles. The envelope authenticates completed provenance.
Build order: inputs → binaries → manifest/checksums → SBOM and deterministic native-input record → provenance → other evidence/envelope → signature.

`sbom-profile.json` freezes a closed CycloneDX **1.5 JSON** subset with required fields and refusal codes.
Document fields are `bomFormat="CycloneDX",specVersion="1.5",version=1,components,dependencies,properties`.
Components require `type,bom-ref,name,version,purl,hashes:[{alg:"SHA-256",content}],licenses:[{expression}],properties`.
Use Cargo metadata from the locked target/profile closure for exact package versions, features and SPDX licence expressions.
`license_file` requires reviewed `LicenseRef` resolution and content digest; missing/UNKNOWN/NOASSERTION licences refuse.
Native/system libraries are separate components with reviewed versions, purls, licences, exact file/SDK hashes and static/dynamic/system linkage.
The CLI (`dctx/cli`), standalone plugin (`dctx/git/standalone`) and default's bundled worker (`dctx/git/bundled`) are distinct components.
The bundled component hashes its containing CLI and names that bom-ref in `dctx:containing-component`; no worker binary is invented.
Rust package components hash their source closure. Root properties bind target/profile/source; role-specific properties are closed in the profile.
Every component has a dependency row `{ref,dependsOn}`, including empty leaves; refs are unique, no dangling edges, CLI depends on its worker.
REL-I checks the complete independent Cargo/native closure, real licence expressions and package URLs; the stdlib checker checks shapes/bindings only.
No Syft, external SBOM generator, random serial number, timestamp or host path enters this deterministic profile.

## 6. Approved inputs and reproducibility

`build-inputs.json` pins stable Rust **1.97.1**, distribution Cargo **1.97.1**, minimal profile and explicit cargo/clippy/rust-std/rustc/rustfmt.
Component versions denote their Rust distribution, not clippy/rustfmt's independent version banners. REL-T owns the exact toolchain file.
Rustc's observation is supplied by the PA; this packet runs neither rustc nor Cargo. Cargo/component agreement must be checked on native builders.
No native runner/linker/SDK version has been approved here; explicit blocked reasons and required pin fields replace guessed versions.
An accepted `native_inputs` record supplies exact approved versions/digests and measured OS/libc floors, bound as a provenance material.

Required settings: `SOURCE_DATE_EPOCH` from supplied exact revision metadata; `--locked --offline --release --target <triple>`;
explicit package/bin/features; `TZ=UTC`, `LC_ALL=C`, no incremental builds; release codegen settings and generic CPU fixed in the recipe.
Apply `--remap-path-prefix` to checkout/build/dependency roots, preserving required flags. No runtime Git, unsafe, network or new external dependency.
Compare two clean builds on independent absolute paths with identical inputs and target/profile. Equality covers unsigned payloads,
manifest, checksum mirror, deterministic SBOM/provenance and optional bundle. No cross-OS/CPU equality claim is made.
Encode generated JSON as ASCII-escaped UTF-8, sorted keys, compact separators and one LF; sort set-like arrays by identity, reject floats/duplicate keys.
The native-input inventory is deterministic and included through material digests; timestamps and acceptance receipts live in separate evidence.
Other evidence/envelopes, receipts, TEST randomness, signatures, notarization and publication timestamps are outside equality; they remain authenticated where used.
macOS UUID/ad-hoc signature behavior and native build scripts require measured evidence; neither epoch nor remapping alone proves reproducibility.

## 7. Refusals and local checks

Refuse missing/unknown/duplicate fields, malformed hashes/sizes, unsafe or case-colliding filenames, mismatched subjects or material/context bindings;
wrong domain/kind/key/signature, stale/revoked/unapproved trust, rollback without authority, TEST promotion, incomplete inventory/licences/features;
unsupported promoted targets, missing or non-native/stale evidence, unmatched profiles/providers, missing exact native pins, or differing unsigned bytes.
`validate_contract.py` uses Python 3 stdlib only, interprets the explicitly supported local schema keywords and refuses unknown schema keywords.
`python3 -I docs/design/plugin-release/validate_contract.py --self-test` builds in-memory positive/mutated negative fixtures; writes nothing.
Use `--release release.json --provenance provenance.json --sbom sbom.cdx.json` to check supplied exact sidecar bytes and their cross-links;
each option also supports a standalone shape check. `--toolchain rust-toolchain.toml` checks the exact pin/components/targets (Python 3.11+).
The validator does not execute binaries, read arbitrary referenced artifacts, verify Ed25519, certify SPDX/purl catalogues, inspect real evidence or promote.
REL-M/I/G/K must perform those production checks using approved implementations. Synthetic self-tests, including a promotable-shaped control,
have no signatures or real evidence and can never authorize signing/publication. Do not interpret validator success as a release gate receipt.
