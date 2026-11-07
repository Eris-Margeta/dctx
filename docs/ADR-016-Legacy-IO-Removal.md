<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# ADR-016: Legacy IO Removal

**Status:** current — architectural principle retained; historical examples describe their original release.

For shipped behavior and limitations, see the [README](../README.md), [testing guide](TESTING.md) and [specification](SPEC.md).

## Context

### The Problem

Legacy core modules mixed filesystem/configuration/state operations with domain behavior; the migration inventory and consumer obligations are recorded in `docs/design/D3-LEGACY-REMOVAL.md`.

### Root Cause Analysis

Deleting wrappers alone would discard compatibility coverage and leave the first-party hash dependency; IO purity and literal dependency-free core ownership are separate obligations (`docs/design/D3-LEGACY-REMOVAL.md`, `docs/design/D3-COMPATIBILITY.md`).

## Decision

Keep `dctx-core` a pure domain with no filesystem, environment, process or global output APIs; its current dependency on `dctx-hash` remains explicit (`crates/dctx-core/src/lib.rs`).

Record the five decision 114 defaults against the open decisions in `docs/design/D3-LEGACY-REMOVAL.md`:

| Decision | Recorded design; evidence/scope |
| --- | --- |
| Removal boundary | Removal in this tree only; downstream/release acceptance is separate (`docs/design/D3-LEGACY-REMOVAL.md`, D3-08/D3-10). |
| Format compatibility | Keep import and byte-identical export against pinned fixtures, including the documented sorted compiled-template encoding (`docs/design/D3-COMPATIBILITY.md`). |
| Alias and Tauri promises | Preserve current behavior as the compatibility target; do not infer completed desktop acceptance from helper migration (`docs/design/D3-LEGACY-REMOVAL.md`, D3-04/D3-05). |
| Existing exports | No implicit deletion on reset/corruption; retain exports (`docs/design/D3-COMPATIBILITY.md`, state family). |
| No-Git boundary | The no-git rule covers installer and benchmark provenance too; D3-06 owns their disposition (`docs/design/D3-LEGACY-REMOVAL.md`). |

## Implementation

### Phase 1: Compatibility Freeze and Codec

D3-01 freezes bytes/facts; D3-02 provides bounded old-format import/export. Fixtures replace a live legacy IO oracle (`docs/design/D3-COMPATIBILITY.md`, `crates/dctx-pack/tests/compatibility.rs`, `crates/dctx-pack/tests/legacy_import.rs`, `crates/dctx-pack/tests/legacy_export.rs`).

### Phase 2: Pure Rendering and Consumer Migration

D3-03 supplies pure legacy snapshot/diff envelopes; D3-04 moves host state/configuration glue into CLI compatibility code (`docs/design/D3-COMPATIBILITY.md`, `crates/dctx-template/tests/legacy_envelopes.rs`, `crates/dctx-cli/src/compat.rs`).

D3-05 covers actual Tauri wiring and D3-06 installer/scripts/provenance; these remain distinct migration obligations from renderer parity (`docs/design/D3-LEGACY-REMOVAL.md`).

### Phase 3: Coverage Migration and Removal

D3-07 migrates rule/parity coverage; D3-08 removes the legacy IO modules and root exports. The current core exposes only domain modules, and the compatibility map records the deleted wrappers and replacement tests (`crates/dctx-core/src/lib.rs`, `docs/design/D3-COMPATIBILITY.md`).

## Results

The retained compatibility oracle covers selection/read denial, text envelopes, binary import/export and CLI reset behavior; core purity checks no longer exempt a legacy directory (`docs/design/D3-COMPATIBILITY.md`, `crates/dctx-core/tests/purity.rs`).

## Trade-offs

### Benefits

Pure rendering consumes injected facts rather than ambient IO, while pinned fixtures preserve historical bytes (`docs/design/D3-COMPATIBILITY.md`).

### Costs

Compatibility glue and fixtures remain maintained outside core; sorted compiled-template encoding is an explicit qualification to historical HashMap ordering (`crates/dctx-cli/src/compat.rs`, `docs/design/D3-COMPATIBILITY.md`).

## Alternatives Considered

Retaining legacy IO wrappers or a live oracle would retain the boundary violation; the chosen replacement uses pure engines, adapters and fixed fixtures (`docs/design/D3-LEGACY-REMOVAL.md`, `docs/design/D3-COMPATIBILITY.md`).

## References

Migration plan: `docs/design/D3-LEGACY-REMOVAL.md`; compatibility record: `docs/design/D3-COMPATIBILITY.md`; current boundary: `crates/dctx-core/src/lib.rs`.

## Lessons Learned

IO-free does not mean literal std-only dependency ownership: core still uses the first-party hash crate (`crates/dctx-core/src/lib.rs`, `docs/CRATE-MAP.md`).

## Future Work

D3-09 decides literal std-only hash ownership; D3-10 owns workspace/release acceptance, including desktop evidence and the maintainer's release decision. This ADR does not close either gate (`docs/design/D3-LEGACY-REMOVAL.md`).
