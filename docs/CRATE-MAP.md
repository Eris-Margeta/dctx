<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Crate map

Static checkout guide for contributors and coding assistants.
40 non-fixture Cargo packages: 36 under `crates/`, one plugin, one desktop,
and two standalone tools. Root `Cargo.toml` includes crates, plugins and desktop;
each tool declares its own workspace. `bindings/README.md` describes planned
bindings; root membership for Python/Node is commented out. Fixture manifests
under tests are not product crates.

## How to read the inventory

Each row gives purpose, layer, direct first-party path dependencies, and direct
external **normal** dependencies. `none` means std-only at that crate boundary;
`first-party only` means no direct external normal dependency, not an assertion
about the complete dependency closure. Dev/build dependencies are excluded.
Optional and target-specific normal dependencies are explicitly marked.
Resolved transitive dependencies and runtime behavior are **unverified** here.

Source convention: for every row named `X`, purpose is taken from
`crates/X/src/lib.rs:1` (first six lines), and dependency names from
`crates/X/Cargo.toml` `[dependencies]`. These two paths are the row's evidence.
Exceptions use the directory shown in the row. Desktop purpose comes from
`apps/dctx-desktop/README.md:3`; its lib header has no purpose description.
Layer labels are navigation classifications of those headers, using the accepted
boundaries in `docs/design/ARCHITECTURE-DECISIONS.md` (DX-ARCH-01).

## Shipped CLI path (v1)

What the `dctx` binary (`crates/dctx-cli`) actually runs, as of v1. No refactor: the other crates stay in the workspace.

- **Shipped:** `dctx-cli` and the `crates/dctx` facade session (index, find, grep, callers, callees, neighbors, skeleton, hubs, pack, read), with dctx-core (selection), dctx-hash, dctx-ports, dctx-pack, dctx-template, dctx-adapter-cache, dctx-adapter-fs (compatibility IO), dctx-map-model (records and rendering) and the `dctx-lex-*` extractors.
- **Request types only:** `dctx-map-query`. The CLI imports `Request`, `Verb`, `Direction`, `Format`, `AnswerBudget` and `EdgeKind`; its `Index` engine is never called.
- **Internal or legacy (compiled as dependencies of `dctx-map-query`, not run by the CLI):** the `dctx-map-query` `Index` engine, `dctx-map-store`, `dctx-map-scan`, `dctx-map-resolve`, `dctx-map-segment`, `dctx-adapter-store`; `dctx-map-parse` likewise. docs/SPEC-MAP-MODE.md sections 6–8 and 10 describe this engine.
- **Unstable (no stability promise in v1):** dctx-plugin-host, dctx-plugin-platform, dctx-plugin-protocol, dctx-plugin-distribution and dctx-desktop-api. The default `dctx-cli` build enables the plugin runtime and the bundled Git plugin process; `--no-default-features --features strict` builds without them.

## Core, engines and adapters

| Crate | Purpose from header | Layer | Direct path dependencies | Direct external normal dependencies |
| --- | --- | --- | --- | --- |
| dctx-hash | First-party SHA-256 content digests. | pure core | none | none (std-only) |
| dctx-core | Pure domain: ids, selection decisions, budgets, errors, pack facts and settings parsing. | pure core | dctx-hash | first-party only |
| dctx-ports | Boundary traits Source, FactStore, Cache, Clock, Cancellation; in-memory implementations and spies. | pure core | dctx-core | first-party only |
| dctx-map-model | IO-free map identities, byte spans, precision, provenance and answers. | pure core | dctx-hash | serde, serde_json (optional `wire-json`, enabled by default) |
| dctx-map-segment | Pure safe codec for immutable map index segments. | pure core | dctx-hash | thiserror |
| dctx-template | Pure DTS v1 renderer and v2 ContextPlan compiler. | mode engine | dctx-core, dctx-hash | first-party only |
| dctx-pack | Pack snapshot and diff engine over injected ports, with no concrete IO. | mode engine | dctx-core, dctx-hash, dctx-ports | first-party only |
| dctx-map-scan | Model-independent scan privacy rules. | mode engine | none | sha2, thiserror |
| dctx-map-parse | Per-file syntax facts; PureParser over lexers, optional tree-sitter oracle. | lexer/parser | dctx-map-model, dctx-lex-rust, dctx-lex-js, dctx-lex-md, dctx-lex-py, dctx-lex-go, dctx-lex-java, dctx-lex-c, dctx-lex-cfg, dctx-lex-cs, dctx-lex-kt, dctx-lex-rb, dctx-lex-php, dctx-lex-swift | sha2, thiserror; optional: tree-sitter, tree-sitter-rust, tree-sitter-md, tree-sitter-javascript, tree-sitter-typescript (`syntax-tree-sitter`) |
| dctx-map-resolve | Pure symbol resolution and move detection. | mode engine | dctx-map-model | thiserror |
| dctx-map-store | Persistent graph facts, pack snapshots and content-addressed comparison cache. | adapter | dctx-core, dctx-hash, dctx-map-model, dctx-ports | rusqlite (`bundled`), thiserror, serde_json |
| dctx-map-query | Incremental indexing and bounded traversal; wires scan, parse, resolve and store APIs. | mode engine | dctx-map-model, dctx-map-scan, dctx-map-parse, dctx-map-resolve, dctx-map-store, dctx-map-segment, dctx-ports, dctx-adapter-fs, dctx-adapter-store, dctx-adapter-cache, dctx-core, dctx-hash | thiserror, serde, serde_json |
| dctx-adapter-fs | Confined filesystem Source adapter; selection before content. | adapter | dctx-hash | thiserror |
| dctx-adapter-store | Filesystem publication of immutable map segments and manifest generations. | adapter | dctx-hash, dctx-map-segment | thiserror |
| dctx-adapter-cache | Cache quotas, leases, expiry, persisted ledger and bounded auto-clean. | adapter | dctx-core, dctx-hash, dctx-ports | thiserror |

## Lexers and structural extractors

Purpose below retains the language and extractor role from each lib header.
Several headers say “zero-dependency”; manifests qualify that statement: the
model-dependent lexers have first-party direct dependencies, and
`dctx-map-model` defaults to external JSON dependencies. Do not infer an
external-free closure from the header wording.

| Crate | Purpose from header | Layer | Direct path dependencies | Direct external normal dependencies |
| --- | --- | --- | --- | --- |
| dctx-lex-rust | Structural Rust extractor built on syn. | lexer/parser | none | proc-macro2, quote, syn, thiserror |
| dctx-lex-js | Hand-written JavaScript and TypeScript structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-md | Hand-written Markdown structure extractor. | lexer/parser | none | none (std-only) |
| dctx-lex-py | Hand-written Python structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-go | Hand-written Go structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-java | Hand-written Java structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-c | Hand-written C and C++ structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-cfg | Hand-written configuration structural extractor (JSON, TOML, YAML). | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-cs | Hand-written C# structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-kt | Hand-written Kotlin structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-rb | Hand-written Ruby structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-php | Hand-written PHP structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |
| dctx-lex-swift | Hand-written Swift structural extractor. | lexer/parser | dctx-hash, dctx-map-model | first-party only |

## Plugin infrastructure, facade and surfaces

| Crate | Purpose from header | Layer | Direct path dependencies | Direct external normal dependencies |
| --- | --- | --- | --- | --- |
| dctx-ed25519 | Ed25519 verification for plugin trust, via audited ed25519-dalek. | pure core | none | ed25519-dalek |
| dctx-plugin-protocol | Std-only codec and typed records for frozen plugin API v1. | plugin | none | none (std-only) |
| dctx-plugin-host | Plugin host domain and brokers; authority, receipts, resources, lifecycle and hooks. | plugin | dctx-hash, dctx-plugin-protocol | first-party only |
| dctx-plugin-platform | Native plugin admission and platform security primitives; general native launches fail closed. | adapter | dctx-hash, dctx-plugin-protocol | thiserror; Linux-only: rustix, landlock, seccompiler (target dependency section) |
| dctx-plugin-distribution | Plugin installer, trust and discovery client: signed manifests, offline bundles, atomic activation. | plugin | dctx-ed25519, dctx-hash, dctx-plugin-protocol | first-party only |
| dctx-plugin-git (`plugins/dctx-plugin-git/`) | Bundled git plugin: local .gitignore protection and brokered git add .gitignore. | plugin | dctx-plugin-protocol | first-party only |
| dctx | Stable library facade for pack, map, templates, cache and optional plugin host. | facade | dctx-adapter-cache, dctx-core, dctx-hash, dctx-lex-js, dctx-lex-md, dctx-map-model, dctx-pack, dctx-ports, dctx-template; optional: dctx-lex-rust, dctx-plugin-host, dctx-plugin-platform, dctx-plugin-protocol | first-party only |
| dctx-cli | Library surface for cache administration; dctx binary wires the same modules. | surface | dctx, dctx-adapter-cache, dctx-core, dctx-map-query, dctx-plugin-distribution, dctx-plugin-protocol, dctx-template; optional: dctx-plugin-git | first-party only |
| dctx-desktop-api | Desktop engine seam for snapshots, settings, recent projects and plugin management. | surface | dctx, dctx-plugin-distribution, dctx-template | serde, serde_json |
| dctx-desktop (`apps/dctx-desktop/src-tauri/`) | Cross-platform GUI for dctx — Directory Context Generator (app README). | surface | dctx-desktop-api, dctx-plugin-distribution, dctx-plugin-protocol | tauri, tauri-plugin-shell, tauri-plugin-dialog, tauri-plugin-clipboard-manager, serde, serde_json, tokio, chrono, dirs |
| architecture-bench (`tools/architecture-bench/`) | Baseline for current dctx map mode. | tool | dctx-map-query | serde, serde_json, sha2, thiserror |
| plugin-index (`tools/plugin-index/`) | Local public-index validator for plugins. | tool | dctx-ed25519, dctx-hash | thiserror |

Tool purpose evidence: each tool's `src/lib.rs:1`; dependencies: its Cargo.toml.
Plugin purpose evidence: `plugins/dctx-plugin-git/src/lib.rs:1`.
Desktop dependencies: `apps/dctx-desktop/src-tauri/Cargo.toml`.
`tauri-build` is a desktop **build** dependency, outside the normal-dependency
columns. Linux plugin dependencies are the one target-specific addition above.
The facade does not directly depend on dctx-map-query: do not turn an intended
architectural boundary into an invented Cargo edge. Its optional Rust extractor
uses `syntax-rust`; plugin runtime dependencies use `plugin-runtime`.

## Dependency picture: core toward consumers

This is a selected-edge tree, not an exhaustive DAG. Children consume parents;
repeated nodes have other inputs listed in the inventory. Every edge shown is
a direct path dependency in the child's manifest. Braces group sibling crates.

```text
dctx-hash
|-- dctx-core
|   |-- dctx-ports
|   |   `-- dctx-pack
|   |       `-- dctx
|   |           |-- dctx-cli
|   |           `-- dctx-desktop-api
|   |               `-- dctx-desktop
|   `-- dctx-template --> {dctx, dctx-cli, dctx-desktop-api}
|-- dctx-map-model
|   |-- dctx-lex-{js,py,go,java,c,cfg,cs,kt,rb,php,swift}
|   |   `-- dctx-map-parse <-- {dctx-lex-rust, dctx-lex-md}
|   |       `-- dctx-map-query
|   |           |-- dctx-cli
|   |           `-- architecture-bench
|   |-- dctx-map-resolve --> dctx-map-query
|   `-- dctx-map-store --> dctx-map-query
|-- dctx-map-segment
|   `-- dctx-adapter-store --> dctx-map-query
|-- dctx-adapter-fs --> dctx-map-query
|-- dctx-adapter-cache --> {dctx, dctx-cli, dctx-map-query}
|-- dctx-plugin-host --> dctx [optional]
|-- dctx-plugin-platform --> dctx [optional]
|-- dctx-plugin-distribution --> {dctx-cli, dctx-desktop-api, dctx-desktop}
`-- plugin-index
dctx-map-scan --> dctx-map-query
dctx-ed25519 --> {dctx-plugin-distribution, plugin-index}
dctx-plugin-protocol
`-- {dctx-plugin-host, dctx-plugin-platform, dctx-plugin-distribution,
     dctx-plugin-git, dctx [optional], dctx-cli, dctx-desktop}
dctx-plugin-git --> dctx-cli [optional]
```

## Language wiring

Authoritative variants and extension selection:
`crates/dctx-map-parse/src/domain/facts.rs:8` (`Language`, `for_path`).
Dispatch: `crates/dctx-map-parse/src/adapters/mod.rs:40` (`PureParser`).
Adapter paths below are relative to `crates/dctx-map-parse/src/adapters/`;
each file imports the named lexer. These are all 13 `dctx-lex-*` packages.

| Language variant(s) | Lexer crate | Adapter |
| --- | --- | --- |
| Rust | dctx-lex-rust | rust.rs |
| JavaScript, TypeScript, Tsx | dctx-lex-js | javascript.rs |
| Markdown | dctx-lex-md | markdown.rs |
| Python | dctx-lex-py | python.rs |
| Go | dctx-lex-go | go.rs |
| Java | dctx-lex-java | java.rs |
| C (C and C++) | dctx-lex-c | c.rs |
| Config (JSON/JSONC, TOML, YAML) | dctx-lex-cfg | config.rs |
| CSharp | dctx-lex-cs | csharp.rs |
| Kotlin | dctx-lex-kt | kotlin.rs |
| Ruby | dctx-lex-rb | ruby.rs |
| Php | dctx-lex-php | php.rs |
| Swift | dctx-lex-swift | swift.rs |
| Unknown | none | mod.rs fallback: module-only |

The optional `TreeSitterParser` in `adapters/mod.rs:74` uses grammars for Rust,
JavaScript/TypeScript/Tsx and Markdown. Other known languages take the pure
adapters; Unknown takes fallback. This is an oracle feature, not the default.

## Cross-cutting rules and evidence locations

- **No Git in the core:** [product contracts](design/PRODUCT-CONTRACTS.md)
  records the core ban and plain-text .gitignore exception. Enforcement evidence:
  `crates/dctx-map-query/tests/no_git.rs` (source spawn audit, PATH trap and
  source-port privacy tests). VCS effects belong to the plugin, not the core.
- **Privacy, selection before content:** `crates/dctx-adapter-fs/tests/privacy.rs`
  uses read spies to check ignored/omitted paths remain unopened;
  `crates/dctx-map-query/tests/source_privacy.rs` checks secrets, ignored files,
  traversal and symlinks, with an included-file positive control.
- **Pure domain boundary:** `crates/dctx-core/src/lib.rs:1` points to
  `crates/dctx-core/tests/purity.rs`, which checks every source module without a
  directory exemption. Comments and unit-test tails are excluded from the IO scan.
  The crate retains its first-party dctx-hash dependency.
- **Plugin PP1–PP10:** [historical PP matrix](archive/design/plugin-evidence/PP-MATRIX.md) maps tests,
  formal evidence and gaps: PP1 authority narrows; PP2 excluded-source privacy;
  PP3 effect audit; PP4 protocol binding; PP5 no blind replay; PP6 strict isolation;
  PP7 bounded resources; PP8 lifecycle progress; PP9 install integrity;
  PP10 hook order/result truth. The matrix is dated scout evidence, not a fresh
  passing test result or certification of today's product acceptance.
- **Legacy removal:** The migration removed the core IO wrappers and their exports.
  [legacy removal inventory](archive/design/D3-LEGACY-REMOVAL.md) remains the historical inventory;
  [compatibility record](archive/design/D3-COMPATIBILITY.md) maps retired families to retained tests.
  Pack codecs, template rendering, filesystem adapters and CLI compatibility IO
  own the replacements. Pure core modules, settings and template assets remain.

## Validation commands

Use [TESTING.md](TESTING.md) for the current commands, toolchains, matrix and
exclusions. Desktop backend checks use `tools/desktop-check.sh`; browser evidence
uses `bun run e2e` from `apps/dctx-desktop`. A skipped desktop check does not
qualify a desktop release. Standalone tools have their own workspaces and checks.
