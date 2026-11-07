Historical design record; superseded by [the current documentation](../../SPEC.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
Current licensing: all first-party dctx code and documentation, including the desktop app and planned GPUI rebuild, are Apache-2.0, copyright Eris Margeta. Licence and pricing discussions below describe historical proposals.

# DX-A3 dependency and licence audit

DX-A3 · 2026-10-16 · Read-only research for ARCHITECTURE-PROPOSAL §1, §6 and §7 Stage 0. This file records what is in the worktree and in the local cargo registry. It does not change any licence, manifest, or lockfile, and it does not approve any crate.

Rules read before writing: [RUST-STANDARDS.md](../../standards/RUST-STANDARDS.md) (approved-dependency list, `forbid(unsafe_code)`, typed errors) and [CODE-GUIDELINES.md](../../standards/CODE-GUIDELINES.md) (one crate per bounded context, ports and adapters, files of about 300 lines, `//!` headers, tests at the bottom). Design inputs: [ARCHITECTURE-PROPOSAL.md](ARCHITECTURE-PROPOSAL.md) and ARCHITECTURE-PROPOSAL-L1-REVIEW.md (not included in the public archive).

`docs/design/dependencies/` is named in the Stage 0 exit row. This packet may write only this file, so the dossiers live here.

## Method

- Direct dependencies are the `[dependencies]`, `[build-dependencies]` and `[dev-dependencies]` tables in each workspace `Cargo.toml` on 2026-10-16.
- “External” means a crates.io package. A `path` dependency is an internal workspace crate.
- Resolved versions come only from the checked-in [Cargo.lock](../../../Cargo.lock). `cargo metadata`, `cargo tree` and network access were not used.
- A licence string is cited only when that exact version’s registry `Cargo.toml` was read under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`. A neighbouring version is never substituted for a missing pin.
- “Unsafe” means an `unsafe` block or `unsafe fn` in that crate’s Rust sources. “C” means the crate compiles C (or equivalent native grammar/SQLite sources), which is distinct from calling the system C library through `libc`.
- The strict profile is the proposal’s contract: zero external production crates and no first-party `unsafe` ([ARCHITECTURE-PROPOSAL.md](ARCHITECTURE-PROPOSAL.md) §1 and the feature table in §2). Dev-dependencies and benches stay outside that promise, as the proposal already says.

## Workspace

[Root Cargo.toml](../../../Cargo.toml) members are `crates/*` and `apps/dctx-desktop/src-tauri`. There is no `[workspace.dependencies]` table and no workspace licence. Bindings are commented out. `bindings/` contains a plan in [bindings/README.md](../../../bindings/README.md), not a crate.

| Crate | Path | Version | Edition | `license` field |
|---|---|---|---|---|
| `dctx-core` | `crates/dctx-core` | 0.25.0 | 2021 | `MIT` |
| `dctx-cli` | `crates/dctx-cli` | 0.25.0 | 2021 | absent |
| `dctx-map-model` | `crates/dctx-map-model` | 0.1.0 | 2024 | absent |
| `dctx-map-scan` | `crates/dctx-map-scan` | 0.1.0 | 2024 | `MIT` |
| `dctx-map-store` | `crates/dctx-map-store` | 0.1.0 | 2024 | absent |
| `dctx-map-parse` | `crates/dctx-map-parse` | 0.1.0 | 2024 | absent |
| `dctx-map-resolve` | `crates/dctx-map-resolve` | 0.1.0 | 2024 | absent |
| `dctx-map-query` | `crates/dctx-map-query` | 0.1.0 | 2024 | absent |
| `dctx-desktop` | `apps/dctx-desktop/src-tauri` | 0.11.0 | 2021 | `MIT` |

No crate under `crates/` has its own `LICENSE` file. The desktop app has [apps/dctx-desktop/LICENSE](../../../apps/dctx-desktop/LICENSE). The root [LICENSE](../../../LICENSE) is MIT. No `[build-dependencies]` table exists under `crates/`. The only build dependency in the workspace is desktop `tauri-build`.

## Lockfile discrepancy

[Cargo.lock](../../../Cargo.lock) (lock version 4) records three workspace packages:

- `dctx-cli` 0.25.0 depends only on `dctx-core`.
- `dctx-core` 0.25.0 depends on `criterion` and `tempfile` (its dev/bench closure).
- `dctx-desktop` 0.11.0 depends on `chrono`, `dctx-core`, `dirs` 5.0.1, `serde`, `serde_json`, `tauri`, `tauri-build`, `tauri-plugin-clipboard-manager`, `tauri-plugin-dialog`, `tauri-plugin-shell` and `tokio`.

No `[[package]]` entry exists for any `dctx-map-*` crate. `rusqlite` and every `tree-sitter*` name are absent. The lock therefore cannot substantiate a resolved normal, build, dev, native or unsafe closure for the landed map manifests. This matches [ARCHITECTURE-PROPOSAL.md](ARCHITECTURE-PROPOSAL.md) §1 and the L1 review (“`Cargo.lock` has no map entries”).

Some versions that map manifests name already appear as desktop or core-dev resolutions. That is a coincidence of the stale graph, not a resolution of the map crates:

| Name in the stale lock | Lock version | Why it is not a map resolution |
|---|---|---|
| `serde` | 1.0.228 | Desktop direct dependency. Query also pins `=1.0.228`, but query is not a lock package. |
| `serde_json` | 1.0.149 | Same situation. Query pins `=1.0.149`. |
| `sha2` | 0.10.9 | Present transitively. Parse pins `=0.10.9`. Scan asks for `0.10` only. |
| `thiserror` | 1.0.69 and 2.0.18 | Both exist in the lock. Map crates ask for major 2. Their packages are not lock members. |
| `tempfile` | 3.24.0 | Core dev-dependency `3`, resolved here. Query pins `=3.24.0` outside the lock. |
| `criterion` | 0.5.1 | Core bench dependency `0.5`, resolved here. |
| `syn` | 1.0.109 and 2.0.114 | Transitive (desktop/criterion graph). Not a dctx parser dependency. |

`proptest` is not in the lock. Regenerating the lock is DX-E6 / the L1 lockfile amendment, in authorized CI. This packet does not refresh it.

## Census

Map manifests, excluding path crates, dev-dependencies and transitives ([ARCHITECTURE-PROPOSAL.md](ARCHITECTURE-PROPOSAL.md) §1):

| Map crate | Direct external production packages | Count |
|---|---|---:|
| model | `serde`, `serde_json` | 2 |
| scan | `sha2`, `thiserror` | 2 |
| store | `rusqlite`, `thiserror`, `serde_json` | 3 |
| parse | `sha2`, `thiserror`, `tree-sitter`, `tree-sitter-rust`, `tree-sitter-md`, `tree-sitter-javascript`, `tree-sitter-typescript` | 7 |
| resolve | `thiserror` | 1 |
| query | `thiserror`, `serde`, `serde_json`, `rusqlite` | 4 |

That is 19 direct external declarations and 10 distinct package names. `dctx-core` and `dctx-cli` add no eleventh production name. Desktop is a separate graph (below).

First-party `unsafe`: every map `lib.rs` and [dctx-cli/src/main.rs](../../../crates/dctx-cli/src/main.rs) declare `#![forbid(unsafe_code)]`. A search of `crates/**/*.rs` found no `unsafe` token in `dctx-core`. `dctx-core`’s header denies production `unwrap` and still permits `expect`. The Rust standard also disallows production `expect` and `panic!`. `forbid(unsafe_code)` covers that crate’s own code, not its dependencies.

## dctx-core

Production dependencies: none. Build dependencies: none.

| Class | Name | Manifest | Resolved in lock | Licence read | Unsafe / C | Strict-profile replacement |
|---|---|---|---|---|---|---|
| dev | `tempfile` | `3` | 3.24.0 | `MIT OR Apache-2.0` (`tempfile-3.24.0/Cargo.toml`) | Unsafe in `src/file/imp/windows.rs`. No C sources in this crate. | Stays a dev-dependency. Production already uses the standard library. |
| dev / bench | `criterion` | `0.5` | 0.5.1 | `Apache-2.0 OR MIT` (`criterion-0.5.1/Cargo.toml`) | Unsafe in `src/lib.rs` and `src/stats/**`. No C compile in this crate. | Stays on the bench `pattern_matching` (`harness = false`). Not part of the shipped profile. |

Strict production closure is already zero external crates. The crate is still an IO core: it exports `walker`, `content_reader`, `registry`, `state`, `writer`, `config` and `git_safety`. Zero dependencies do not make it the pure domain in the proposal.

## dctx-cli

Production path dependencies: `dctx-core`, `dctx-map-query`. Build dependencies: none. Dev-dependencies: none.

There is no direct external crate. The binary still does not have the original zero-external closure, because `dctx-map-query` is an unconditional dependency ([crates/dctx-cli/Cargo.toml](../../../crates/dctx-cli/Cargo.toml)). The TUI remains standard-library code under [ADR-012](../../ADR-012-Zero-Dependency-TUI.md).

Strict-profile replacement: link the future facade with the strict feature set, and keep `dctx-map-query`’s current SQLite and tree-sitter closure on an explicit enhanced profile. Until that split exists, any CLI build that follows today’s manifest pulls query’s external crates. The checked-in lock still describes the old CLI that linked only `dctx-core`.

## dctx-map-model

Path dependencies: none. Build dependencies: none.

| Class | Name | Manifest | Resolved | Licence read | Unsafe / C | Strict-profile replacement |
|---|---|---|---|---|---|---|
| production | `serde` | `1`, feature `derive` | not a lock member; desktop lock has 1.0.228 | 1.0.228 is `MIT OR Apache-2.0` | 1.0.228 has `unsafe` in `src/core/format.rs` and `src/core/ser/impls.rs`. Its `build.rs` writes a private module and checks `rustc`; it does not compile C. | Remove derives from the pure model. Callers that need JSON use the facade wire adapter. |
| production | `serde_json` | `1` | not a lock member; desktop lock has 1.0.149 | 1.0.149 is `MIT OR Apache-2.0` | 1.0.149 has `unsafe` in `src/raw.rs`, `src/read.rs`, `src/ser.rs` and `src/value/mod.rs`. `build.rs` only sets a `fast_arithmetic` cfg. | Same wire adapter. Text rendering can stay on `std::fmt`. |
| dev | `proptest` | `1` | absent from the lock | not read for a resolved version | not established for the version CI will select | Remains a dev-dependency for budget invariants. |

`serde` / `serde_json` are used for ids, graph records, answers and the JSON renderer (`domain/ids.rs`, `domain/graph.rs`, `domain/answer.rs`, `adapters/render.rs`). The model also contains a `pub(crate)` SHA-256 in `domain/sha256.rs` with no external crate. That function is the stable-id alias, not a shared digest API.

Strict replacement: an IO-free model with no serde types on the public boundary. Golden JSON conversion moves to the wire adapter (proposal DX-B1).

## dctx-map-scan

Path dependencies: none. It does not depend on `dctx-map-model` ([SPEC-MAP-MODE.md](../../SPEC-MAP-MODE.md) §13a).

| Class | Name | Manifest | Resolved | Licence read | Unsafe / C | Strict-profile replacement |
|---|---|---|---|---|---|---|
| production | `sha2` | `0.10` | not a lock member; lock contains 0.10.9 via another graph | 0.10.9 is `MIT OR Apache-2.0`. Description in that file: “Pure Rust implementation of the SHA-2 hash function family”. | See the `sha2` dossier below. | A first-party SHA-256 with published vectors, or the L1 exception if Eris accepts it. Scan cannot call the model’s private digest today. |
| production | `thiserror` | `2` | not a lock member | pin `2` is not an exact crate; 2.0.18 is in the lock but its sources were not in the registry | not established for the unresolved 2.x crate | A `std` error enum with `Display` and `From`. |
| dev | `tempfile` | `3` | not a lock member of this crate; lock has 3.24.0 | 3.24.0 as cited under `dctx-core` | as cited under `dctx-core` | Stays dev-only. Tests already use it for ignore parity. |

`sha2` is used in `domain/scanned_file.rs` as `Sha256::digest` of authorized bytes.

## dctx-map-store

Path dependency: `dctx-map-model`.

| Class | Name | Manifest | Resolved | Licence read | Unsafe / C | Strict-profile replacement |
|---|---|---|---|---|---|---|
| production | `rusqlite` | `0.32`, feature `bundled` | absent from the lock | not read; `rusqlite-0.32*` is not in the local registry | C boundary is the `bundled` feature. L1 accepted “rusqlite bundles C” from the manifests. This packet did not reopen the 0.32 sources. | Leave this crate as the SQLite bridge. The strict default uses segment storage instead of replacing SQLite inside this crate. |
| production | `thiserror` | `2` | not a lock member | not read for the unresolved pin | not established | `std` error enum in the store, or keep `thiserror` only on the enhanced SQLite profile. |
| production | `serde_json` | `1` | not a lock member | 1.0.149 cited above if that is the version CI selects | cited above for 1.0.149 | Node and edge payloads become segment sections. Schema today stores JSON text in `nodes.payload` and `edges.payload` (`adapters/schema.rs`). |
| dev | `proptest` | `1` | absent | not read for a resolved version | not established | Stays dev-only. |

`adapters/schema.rs` creates SQLite tables plus `CREATE VIRTUAL TABLE fts USING fts5`. That is the FTS5 comparison oracle. Strict mode does not reimplement FTS5; it ships a sorted dictionary and postings (proposal §3) and keeps this crate as `store-sqlite`.

## dctx-map-parse

Path dependency: `dctx-map-model`.

| Class | Name | Manifest | Resolved | Licence read | Unsafe / C | Strict-profile replacement |
|---|---|---|---|---|---|---|
| production | `sha2` | `=0.10.9` | package not in the lock; 0.10.9 happens to be a lock entry | `MIT OR Apache-2.0` for 0.10.9 | dossier below | Same digest decision as scan. `body_fingerprint` in `domain/identity.rs` hashes whitespace-normalized bodies. |
| production | `thiserror` | `=2.0.18` | 2.0.18 is a lock name, parse is not a lock package, and `thiserror-2.0.18` sources were not in the registry | not read | not established for 2.0.18 | `std` error enum. `ParseError` currently derives `thiserror::Error`. |
| production | `tree-sitter` | `=0.25.10` | absent | not read | C runtime plus Rust FFI. L1: “tree-sitter and its grammars are C”, from the manifests. Sources were not in the registry. | Removed from the strict extractor. Kept behind `syntax-tree-sitter`. |
| production | `tree-sitter-rust` | `=0.24.0` | absent | not read | Generated C grammar, same L1 finding. | Structural Rust extractor, or optional `syn`. Grammar stays the oracle. |
| production | `tree-sitter-md` | `=0.3.2` | absent | not read | Generated C grammar. | Hand-written Markdown lexer. |
| production | `tree-sitter-javascript` | `=0.25.0` | absent | not read | Generated C grammar. | Hand-written JS lexer, including JSX. |
| production | `tree-sitter-typescript` | `=0.23.2` | absent | not read | Generated C grammar. One crate supplies TypeScript and TSX (`LANGUAGE_TYPESCRIPT`, `LANGUAGE_TSX`). | Hand-written TS/TSX lexer. |
| dev | `proptest` | `1` | absent | not read for a resolved version | not established | Stays dev-only. |

The parser calls `tree_sitter::Parser`, `tree_sitter_rust::LANGUAGE`, `tree_sitter_javascript::LANGUAGE`, `tree_sitter_typescript::LANGUAGE_TYPESCRIPT`, `tree_sitter_typescript::LANGUAGE_TSX`, and `tree_sitter_md::LANGUAGE` / `INLINE_LANGUAGE`.

## dctx-map-resolve

Path dependency: `dctx-map-model`.

| Class | Name | Manifest | Resolved | Licence read | Unsafe / C | Strict-profile replacement |
|---|---|---|---|---|---|---|
| production | `thiserror` | `2` | not a lock member | not read | not established | `std` error enum for `ResolveError`. |
| dev | `proptest` | `1` | absent | not read for a resolved version | not established | Stays dev-only (`domain/moves.rs`). |

No C and no parser crate. A strict resolver stays pure once `thiserror` is replaced or accepted as a named exception. The current Rust standard already lists `thiserror` on the approved list; the strict profile in the proposal is narrower than that list.

## dctx-map-query

Path dependencies: `dctx-map-model`, `dctx-map-scan`, `dctx-map-parse`, `dctx-map-resolve`, `dctx-map-store`.

| Class | Name | Manifest | Resolved | Licence read | Unsafe / C | Strict-profile replacement |
|---|---|---|---|---|---|---|
| production | `thiserror` | `=2.0.18` | same gap as parse | not read | not established | `std` error enum. `domain/error.rs` maps `rusqlite::Error` and `serde_json::Error` into query errors. |
| production | `serde` | `=1.0.228`, feature `derive` | pin matches the desktop lock version; query is not locked | `MIT OR Apache-2.0` for 1.0.228 | unsafe in 1.0.228, cited above | Cached file payloads (`domain/cached.rs`) move to segment types. The engine stops naming serde. |
| production | `serde_json` | `=1.0.149` | pin matches the desktop lock version; query is not locked | `MIT OR Apache-2.0` for 1.0.149 | unsafe in 1.0.149, cited above | JSON remains a projection at the process boundary, not the hot path. |
| production | `rusqlite` | `=0.32.1`, feature `bundled` | absent from the lock and from the local registry | not read | `bundled` compiles SQLite C. Direct use in `adapters/database.rs` (WAL readers, FTS, `bm25`). | Query talks to a `FactStore` port. SQLite stays inside `dctx-map-store` on `store-sqlite`. |
| dev | `tempfile` | `=3.24.0` | 3.24.0 is the core lock version; query is not locked | `MIT OR Apache-2.0` for 3.24.0 | unsafe on Windows, cited above | Stays dev-only. |

Query repeats `rusqlite`, `serde` and `serde_json` even though store and model already depend on them. Feature-gating store is not enough: this manifest links SQLite and JSON itself.

## sha2 0.10.9 dossier

Read from `sha2-0.10.9` in the local registry.

- Licence: `MIT OR Apache-2.0`.
- Default features: `default = ["std"]`. The `asm` feature is optional and turns on the `sha2-asm` dependency. `sha2-asm` 0.6.1 was not in the local registry, so its sources were not classified.
- Unsafe remains with `asm` off. `src/sha256.rs` `compress256` always contains `unsafe { core::slice::from_raw_parts(...) }`. On `x86` / `x86_64`, the crate uses `src/sha256/x86.rs` (unsafe intrinsics) unless `force-soft` or `force-soft-compact` is set. `src/sha256/soft.rs` has no `unsafe` token.
- No C file is compiled by `sha2` itself at default features. Optional `asm` pulls `sha2-asm`.

L1 amendment 2 offers two choices and recommends the second only when “zero external crates” must be literal: (a) allow `sha2` with the asm feature off as an audited exception; (b) a first-party implementation of about 200 lines checked against NIST vectors. This packet does not choose. Choice (a) is pure Rust in the sense of no C compile, and it is still an external crate that contains `unsafe`. `dctx-map-model` already has a private SHA-256; promoting it, or writing the scan/parse digest beside it, is the literal-zero replacement. It needs vectors and differential tests before it can replace `sha2`. The known vector in `dctx-map-parse` is `body_fingerprint("abc") == ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

## tree-sitter and rusqlite dossiers

`tree-sitter` `=0.25.10`, `tree-sitter-rust` `=0.24.0`, `tree-sitter-javascript` `=0.25.0`, `tree-sitter-typescript` `=0.23.2` and `tree-sitter-md` `=0.3.2` are exact pins in [crates/dctx-map-parse/Cargo.toml](../../../crates/dctx-map-parse/Cargo.toml). They are absent from `Cargo.lock` and from the local registry, so this packet did not read their `license` fields or count `unsafe` blocks inside them.

What is established without those tarballs:

- [SPEC-MAP-MODE.md](../../SPEC-MAP-MODE.md) §13a D1 (L1, 2026-10-16) adopts `tree-sitter` with the Rust, JavaScript, TypeScript and Markdown grammars for `dctx-map-parse`. The older §13 text says the hand-written alternative gives poor TS/JS edges and is not recommended. The architecture proposal revisits that settlement: structural extractors become the strict default only after coverage gates, and tree-sitter stays available.
- [RUST-STANDARDS.md](../../standards/RUST-STANDARDS.md) dctx additions name `tree-sitter`, `tree-sitter-rust` and `tree-sitter-md` with exact pins. They do not name `tree-sitter-javascript` or `tree-sitter-typescript`. The manifests and §13a do. That is a standards-text gap, not a new approval from this packet.
- L1’s review of the architecture proposal marked “rusqlite bundles C; tree-sitter and its grammars are C” as true from the manifests.
- Landed parse code reaches those crates through `tree_sitter::Language`, `Parser`, and `Node`.

`rusqlite` `0.32` (store) and `=0.32.1` (query), both with `features = ["bundled"]`, are also absent from the lock and the registry. Cached `rusqlite-0.40.2` declares `license = "MIT"` and was not used as the licence of 0.32. Cached `libsqlite3-sys-0.38.2` (a later binding) declares `license = "MIT"`, has a `bundled` feature, and its `build.rs` compiles `sqlite3.c`. That shows what `bundled` means on a newer sys crate. It is not a file listing of rusqlite 0.32.1.

No mmap crate, Git crate, PyO3, napi-rs, GPUI or `syn` dependency is present in any dctx manifest. Those remain proposals. Their dossiers are the “not in tree” section below. No approval is recorded here.

## Strict profile: what must be replaced

A strict engine that can be built from today’s crates does not exist. The production edges to remove, and the first-party stand-in for each, are:

| External capability | Crates that link it today | Replacement in the strict profile |
|---|---|---|
| JSON derive and `serde_json::Value` / `to_string` | model, store, query | Facade wire adapter. Domain and segments use their own types. |
| `thiserror` derives | scan, store, parse, resolve, query | Hand-written `Error` + `Display` + `From` per crate, which the Rust standard allows `thiserror` to replace only when the strict promise is relaxed. |
| SHA-256 of file bytes and body fingerprints | scan, parse | First-party digest with NIST vectors, shared by scan and parse without an IO crate. L1’s `sha2` exception is the alternative if literal zero is declined. |
| SQLite, WAL, FTS5, `bm25` | store and, separately, query | `dctx-map-segment` sorted dictionary and postings; buffered reads; integer scores. `dctx-map-store` remains `store-sqlite`. |
| Tree-sitter runtime and four grammars | parse, pulled into query and the CLI | Safe structural extractors in `dctx-map-parse`. Native adapters move to `dctx-adapter-syntax` under `syntax-tree-sitter`. |
| `syn` | not linked | Optional `syntax-rust` only after a dossier. It does not remove the JS, TS or Markdown lexers. |

Internal path edges that a strict CLI still needs: core (after the pure-domain split), model, scan, parse, resolve, query, segment codec, and the fs/cache adapters written in safe Rust. `dctx-map-store` drops out of that closure.

Dev-dependencies that the strict promise does not remove: `tempfile`, `criterion`, `proptest`.

## Approved list versus the manifests

[RUST-STANDARDS.md](../../standards/RUST-STANDARDS.md) approves, for every Rust packet, `serde`, `serde_json`, `thiserror`, `sha2`, `hex`, `rusqlite`, `clap` and `regex`, plus dev `proptest` and `tempfile`. The dctx addition approves `rusqlite` with `bundled` in `dctx-map-store`, and `tree-sitter`, `tree-sitter-rust` and `tree-sitter-md` in `dctx-map-parse`. It also says “everything `dctx-core` already uses”, which covers core’s dev pair `tempfile` and `criterion`.

Consequences, with no new approval added:

- `hex`, `clap` and `regex` are approved and unused as direct dependencies.
- `serde`, `serde_json`, `thiserror`, `sha2`, `rusqlite` and the three named tree-sitter crates are approved and used. Approval does not put them in the strict profile.
- `tree-sitter-javascript` `=0.25.0` and `tree-sitter-typescript` `=0.23.2` are used and are adopted by SPEC-MAP-MODE §13a. They are missing from the standards bullet that lists grammar crates.
- `rusqlite` is approved for `dctx-map-store` and is also a direct dependency of `dctx-map-query`.
- `syn` is not approved. The proposal lists it as a capability that still needs a dossier.
- Desktop crates (`tauri`, plugins, `tokio`, `chrono`, `dirs`, `tauri-build`) are outside the place suite (`cargo test --workspace --exclude dctx-desktop`) and are not on the approved list.

## Licence discrepancy

No licence file or `license` field was edited.

### What the accepted ADR and the README say

[ADR-013](../../ADR-013-Monorepo-Structure.md) (Accepted, 2026-01-25):

- §1 splits “Open-source components (MIT)” from “Commercial components (Proprietary)”.
- §4 directory tree labels `apps/dctx-desktop/` as “PROPRIETARY - Tauri GUI ($29)” and says its `LICENSE` is “PROPRIETARY”.
- §4 licensing model: MIT for `dctx-core`, `dctx-cli` and future bindings; proprietary for `apps/dctx-desktop/` with “$29 one-time payment” and “14-day trial period”; “Root `LICENSE` file applies to” the MIT components.
- §7 shows a sample desktop manifest `license = "PROPRIETARY"` and says the desktop `LICENSE` contains proprietary terms.
- The checklist marks “Create proprietary LICENSE for desktop app” as done.

[README.md](../../../README.md) architecture section repeats that picture: `crates/` as “MIT License”, `apps/dctx-desktop/` as “PROPRIETARY - $29”, and a “Commercial (Proprietary)” bullet for the desktop GUI at “$29 one-time”. The same README’s badge is `license-MIT` and its closing licence section says “MIT License. See LICENSE for details.”

[apps/dctx-desktop/README.md](../../../apps/dctx-desktop/README.md) says both “License: MIT - Free and Open Source” at the top and, in “Licensing”, “This application is PROPRIETARY software”, “You may NOT redistribute or modify”, and “$29 one-time”, and it points at `LICENSE` for “full terms”.

### What the licence files and manifests say

- [LICENSE](../../../LICENSE) begins “MIT License”, copyright 2026 Eris Margeta, and grants the right to “use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies”.
- [apps/dctx-desktop/LICENSE](../../../apps/dctx-desktop/LICENSE) is the same MIT grant, not a proprietary terms file.
- [apps/dctx-desktop/src-tauri/Cargo.toml](../../../apps/dctx-desktop/src-tauri/Cargo.toml) sets `license = "MIT"`.
- [crates/dctx-core/Cargo.toml](../../../crates/dctx-core/Cargo.toml) and [crates/dctx-map-scan/Cargo.toml](../../../crates/dctx-map-scan/Cargo.toml) set `license = "MIT"`.
- `dctx-cli`, `dctx-map-model`, `dctx-map-store`, `dctx-map-parse`, `dctx-map-resolve` and `dctx-map-query` omit `license`.
- [bindings/README.md](../../../bindings/README.md) assigns MIT to planned Python and Node bindings. There is no binding crate to license.

The L1 review recorded this contradiction as true and left it as a decision for the maintainer. The proposal’s recommendation (§6 and decision 11) is MIT for the libraries, CLI, bindings and existing desktop code, with any proprietary claim limited to newly authored GPUI UI after a separate review. This audit does not adopt that recommendation and does not revoke the MIT grants already present in the two `LICENSE` files.

## Parser direction

Landed extraction is `TreeSitterParser` ([crates/dctx-map-parse/src/lib.rs](../../../crates/dctx-map-parse/src/lib.rs)). Language follows the final extension: Rust (`.rs`); JavaScript (`.js`, `.mjs`, `.cjs`, `.jsx`); TypeScript (`.ts`, `.mts`, `.cts`); TSX (`.tsx`); Markdown (`.md`, `.markdown`); anything else is `Unknown`.

Facts produced today, which a replacement must keep or explicitly drop:

- One module node per file, plus symbols, tests and doc sections.
- Rust symbol kinds from grammar nodes: function, method, struct, union, enum, trait, impl, const, static, type, associated type, macro definition. Macro bodies are not expanded (`macro_bodies_are_not_expanded_into_false_symbols`).
- JS/TS kinds: function, class, abstract class, method, interface, type alias, enum, and test names taken from call expressions.
- Markdown: ATX and setext headings as `NodeKind::Doc`, nested by level. Fenced code, indented code and HTML blocks are not descended into. Inline grammar emits unresolved `Documents` links.
- Edges and raw references: `Contains`, `Imports`, `Implements`, `Calls`, `Tests`, `Reexports`, `UsesType`, `Documents`, with scope, alias and line span.
- Unknown files emit only the module node and a comment summary. References stay empty.
- Rust and JS/TS return `ParseError::Syntax` when the tree `has_error`, and they do not emit partial facts (`fn broken(` is a fixture). Markdown does not perform that check. Tree-sitter’s error recovery is present in the library and unused for Rust and JS/TS, because the adapter refuses the tree.

### What the strict direction adds

- A first-party structural Rust extractor and hand-written lexers for JavaScript, TypeScript, TSX and Markdown, with zero external production crates and zero first-party `unsafe`.
- Explicit capability labels. Approximate extractors emit unresolved or unknown instead of a confident edge (proposal §3).
- Coverage that the proposal says must be stated, and that today’s grammar walk largely gets from tree-sitter: declarations, imports, headings and conservative calls. Gaps to carry as non-goals until measured: Rust macro expansion, TypeScript generics, JSX, regex and template literals, and malformed files.
- The same `ParsedFile` / `RawReference` records, so resolve and the fixtures stay stable.
- Limits already sketched in code (`TooLarge` at `u32::MAX` bytes, duplicate-ordinal overflow) restated as cancellation and refusal tests (DX-B4).
- A digest for `body_fingerprint` that does not require `sha2`, if the strict profile is literal.

Planning envelopes in the proposal (size, throughput, precision, recall) are hypotheses. This audit measured none of them.

### What the strict direction removes

- The five parser packages and their C / FFI boundary from the default closure.
- `tree_sitter::Node` from `facts.rs`, `rust.rs`, `rust_items.rs`, `rust_references.rs`, `javascript.rs`, `javascript_items.rs`, `javascript_references.rs` and `markdown.rs`.
- `ParseError::Grammar` (`tree_sitter::LanguageError`) and `ParseError::NoTree` from the strict error surface.
- The current all-or-nothing syntax refusal, if structural mode is required to return partial facts for malformed files. That is a behavior change. The tree-sitter adapter can keep the refusal as its own oracle behavior.
- Any claim that strict mode has tree-sitter’s error-tolerant syntax. SPEC-MAP-MODE §13 warned that hand-written TS/JS edges are the weaker option. The proposal makes that weakening acceptable only after the held-out corpus gates.

### What optional `syn` adds and does not add

`syn` is not a direct dependency. Lock entries `syn` 2.0.114 and 1.0.109 are transitive. Registry `syn-2.0.114/Cargo.toml` says `license = "MIT OR Apache-2.0"`. That version contains `unsafe` in `src/buffer.rs` (cursor pointer arithmetic). Transitive crates (`quote`, `proc-macro2`, and the rest of a parser feature) were not inventoried.

Adding `syn` on `syntax-rust` would add an external Rust closure that must be audited for unsafe rather than assumed safe because it is Rust (proposal §3). It would parse supported Rust syntax and spans. It would not type-check, expand macros, or parse JS, TS or Markdown. It often rejects incomplete files, so the structural extractor or the tree-sitter adapter remains the fallback. No version of `syn` is approved in RUST-STANDARDS.md.

### What optional tree-sitter keeps

The current pins, fact walk and fixtures stay as the rich mode and as the oracle for DX-B4. The proposal’s `syntax-tree-sitter` feature is the disclosed C/unsafe bundle. It is not the strict default. Removing it from the default closure waits until a candidate passes the coverage gates. This audit does not switch that default.

## Desktop appendix

Excluded from the crate-by-crate strict census except here. Manifest: [apps/dctx-desktop/src-tauri/Cargo.toml](../../../apps/dctx-desktop/src-tauri/Cargo.toml). Resolved versions are the lock versions for `dctx-desktop` 0.11.0. Exact-version licence files for several of these pins were not in the local registry (`tauri-2.10.1`, `tokio-1.49.0`, `chrono-0.4.43`, and the three plugins’ lock versions). Their SPDX lines are therefore not stated.

| Class | Name | Manifest | Lock version | Licence read for that version | Unsafe / C from sources of that version |
|---|---|---|---|---|---|
| production | `dctx-core` | path | 0.25.0 | crate field `MIT` | no `unsafe` token in `crates/dctx-core` |
| production | `tauri` | `2`, empty features; `custom-protocol` enables `tauri/custom-protocol` | 2.10.1 | not read | not re-audited; native UI stack, outside the strict engine |
| production | `tauri-plugin-shell` | `2` | 2.3.5 | not read | not re-audited |
| production | `tauri-plugin-dialog` | `2` | 2.6.0 | not read | not re-audited |
| production | `tauri-plugin-clipboard-manager` | `2` | 2.3.2 | not read | not re-audited |
| production | `serde` | `1`, feature `derive` | 1.0.228 | `MIT OR Apache-2.0` | unsafe in 1.0.228, cited above |
| production | `serde_json` | `1` | 1.0.149 | `MIT OR Apache-2.0` | unsafe in 1.0.149, cited above |
| production | `tokio` | `1`, feature `rt-multi-thread` | 1.49.0 | not read | not re-audited |
| production | `chrono` | `0.4`, feature `serde` | 0.4.43 | not read | not re-audited |
| production | `dirs` | `5` | 5.0.1 | `MIT OR Apache-2.0` | no `unsafe` token in `dirs` 5.0.1 itself; it depends on `dirs-sys` 0.4.1, which has `unsafe` in `src/lib.rs` and whose own licence field is `MIT OR Apache-2.0` |
| build | `tauri-build` | `2`, empty features | 2.5.4 | not read | build-time only; sources of 2.5.4 were not in the registry |

Desktop Rust sources under `src-tauri/src` contain no `unsafe` token. They link Tauri. A zero-external-crate profile does not apply to this GUI. The proposal keeps Tauri, later points it at the facade, and treats GPUI as a separate presentation crate (`apps/dctx-desktop-gpui`) whose licence is undecided. Frontend npm dependencies in `apps/dctx-desktop/package.json` are outside this Rust audit.

`dirs` is the concrete home-directory discovery the proposal keeps out of a library session. A strict engine must not gain `dirs` by linking the desktop crate.

## Not in the tree

| Proposed capability | Status in this worktree | Strict-profile meaning |
|---|---|---|
| `syn` | not a direct dependency; transitive `syn` 2.0.114 is `MIT OR Apache-2.0` and contains unsafe | optional Rust syntax only |
| mmap | no crate | `std` has no safe portable mmap; strict uses buffered reads or `Arc<[u8]>` |
| PyO3 / maturin | `bindings/dctx-py/` does not exist | binding graph, audited separately |
| napi-rs | `bindings/dctx-node/` does not exist | binding graph, audited separately |
| C ABI | `bindings/dctx-ffi/` does not exist | needs an explicit FFI exception under `forbid(unsafe_code)` |
| GPUI | `apps/dctx-desktop-gpui/` does not exist | UI licence still open |
| Git crate | no crate; revision code spawns `git` | `git-process` stays optional; a native crate needs its own dossier |
| FST | no crate | not in the first segment implementation |

## Checks this packet did not run

`cargo metadata`, `cargo tree`, `cargo deny` and the workspace test suite were not run. The place suite remains `cargo test --workspace --exclude dctx-desktop` on the VM. Resolved transitive counts, target-specific cfg sets, and an SBOM wait for the CI lock refresh. Counts above are direct manifest declarations only.
