# ADR-017: Plugin System

**Status:** accepted (PA, decision 114, pending the maintainer confirmation)

## Context

### The Problem

Plugins need independent installation and effects without restoring Git or plugin execution to the pure core (`docs/design/PLUGINS.md`).

### Root Cause Analysis

A separate process isolates crashes but supplies no security sandbox by itself; confinement and brokered authority require distinct platform evidence (`docs/design/PLUGINS.md`, `crates/dctx-plugin-platform/README.md`).

## Decision

Use one out-of-process mechanism: bounded, versioned, length-framed UTF-8 JSON over stdio. Bundled plugins use a child of the same executable; installed plugins use external artifacts speaking the same protocol (`docs/design/PLUGINS.md`).

Bundle `plugins/dctx-plugin-git` in the default composition, with disable support and explicit consent. Its responsibility is local `.gitignore` protection and broker requests for staging, not core hashing/indexing/cache behavior (`docs/design/PLUGINS.md`, `crates/dctx-cli/Cargo.toml`).

Installation, enablement, verification and consent are independent: installed/enabled/disabled, verified/quarantined, and granted/needs-consent/policy-denied/unsupported. A copied or migrated binary without consent is enabled/needs-consent; disable persists across upgrades (`docs/design/PLUGINS.md`).

Effective grants intersect manifest requests, user consent, organization and host ceilings, current selection/provenance and enforceable adapter operations. Configuration cannot supply new consent; authority is checked at admission and commit (`docs/design/PLUGINS.md`, `docs/design/plugin-evidence/PP-MATRIX.md`).

## Implementation

### Phase 1: Protocol, Host and Distribution

Protocol, broker, platform and distribution have separate crates; the Git plugin is a distinct crate (`docs/CRATE-MAP.md`, `docs/design/PLUGINS.md`).

### Phase 2: Strict Composition

The strict profile selects pack/map/templates/fs/segments/cache without `plugin-runtime` or `git-worker`. CLI features are additive: use `--no-default-features --features strict`; the facade runtime dependencies are optional (`crates/dctx-cli/Cargo.toml`, `crates/dctx/Cargo.toml`).

This feature wiring is not a claim that all strict release closure and zero-spawn gates have passed; the acceptance matrix identifies the required independent audits (`docs/design/plugin-evidence/PP-MATRIX.md`).

## Results

The following are the matrix's source-inspection statuses, not fresh test results. All ten have finite formal/component evidence and incomplete product acceptance (`docs/design/plugin-evidence/PP-MATRIX.md`).

| Property | Matrix status | Remaining acceptance obligation (same matrix) |
| --- | --- | --- |
| PP1 authority only narrows | partly | Independent current authority inputs through a running worker. |
| PP2 no excluded source access | partly | Real broker/Git/native privacy observations. |
| PP3 effect audit | partly | Durable intent/completion with crash recovery around real effects. |
| PP4 protocol binding | partly | Verified launch/runtime identity and real executable sessions. |
| PP5 no blind replay | partly | Durable ledger reconstruction and crash-cut replay controls. |
| PP6 strict isolation | partly | Separate strict/default closure and process audits. |
| PP7 bounded resources | partly | Real child/host quotas and aggregate allocation evidence. |
| PP8 lifecycle progress | partly | Native supervision, progress, cancellation and reap evidence. |
| PP9 install integrity | partly | Product crash/concurrency and native storage qualification. |
| PP10 hook order and result truth | partly | Real ordered workers and receipt-derived product results. |

## Trade-offs

### Benefits

The same protocol supports one default executable and independent external plugins, with a plugin-free engine composition (`docs/design/PLUGINS.md`, `crates/dctx/Cargo.toml`).

### Costs

Startup, framing and copying add overhead; native confinement remains target-specific and requires qualification (`docs/design/PLUGINS.md`, `crates/dctx-plugin-platform/README.md`).

## Alternatives Considered

The design declines native dynamic libraries, defers Wasm, and treats declarative recipes as insufficient for a general plugin API (`docs/design/PLUGINS.md`).

## References

Contract: `docs/design/PLUGINS.md`; acceptance evidence: `docs/design/plugin-evidence/PP-MATRIX.md`; native limits: `crates/dctx-plugin-platform/README.md`.

## Lessons Learned

Component tests and finite formal coverage do not establish real native execution or release acceptance (`docs/design/plugin-evidence/PP-MATRIX.md`).

## Future Work

Native launch/confinement is deferred under the unsafe restriction: reviewed launch APIs require unsafe callers, and no target is advertised as a qualified launcher. Unsupported stays typed; there is no unrestricted fallback (`crates/dctx-plugin-platform/README.md`).

Release tooling, real conformance corpus, strict closure audits, desktop packaging evidence and isolated benchmarks remain release gates; the matrix's proposed tooling is not evidence of a completed run (`docs/design/plugin-evidence/PP-MATRIX.md`, `docs/design/PLUGINS.md`).
