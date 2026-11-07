Historical design record; superseded by [the current documentation](../../design/PRODUCT-CONTRACTS.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx product contracts

DX-A0b · contract revision 1 · 2026-10-16.
DX-A0c applies decision 078 and decision 079 on top of this contract. Where they conflict with decision 077, decision 078 wins.
Defines the successor product obligations for DX-ARCH Stage 0 under decision 077.
Depends on the proposal, L1 review and local engineering standards.
Used by implementation packets, formal models and release conformance checks.

## Authority and scope

decision 077 overrides conflicting recommendations in [DX-ARCH](ARCHITECTURE-PROPOSAL.md)
and [SPEC-MAP-MODE](../../SPEC-MAP-MODE.md), including the tree-sitter default.
decision 078 (Eris, 16.10.2026), verbatim, supersedes every remaining allowance of git
as a fast path, evidence source, or metadata source:

> fix privacy leak - let's not use git for anything. it is a dependancy in runtime - we do not want that. CROSS GIT for good. we will not be depending on it.

Content identity, freshness, view and revision identity, cache ownership and
expiry use dctx's own algorithm-tagged digest (first-party SHA-256,
`crates/dctx-hash`) and file metadata only. The core never reads `.git/`, never
spawns git, and never uses git object ids. Any VCS-aware behaviour belongs to a
plugin through the plugin API. The core keeps reading `.gitignore` files as
plain-text ignore rules (privacy). decision 079: git behaviour (`.gitignore` edit,
`git add`) exists only as the default-shipped plugin `dctx-plugin-git`
(`docs/design/PLUGINS.md` once it lands), never in the core.

The [decision record](ARCHITECTURE-DECISIONS.md) separates
the maintainer's accepted direction from proposed details and outstanding approvals.
“Must” below specifies the successor contract, not a claim about landed code.
This documentation packet changes no runtime behavior, dependencies or licences.

The L1 review (not included in the public archive) remains binding where decision 077
and decision 078 do not supersede it. DX-P1 is replaced by DX-P1c (landed): the core
does not use git. DX-P2 adds bounded cache status/GC; authorized VM CI must
resolve the stale lockfile and provide suite evidence. DX-P2 and the lockfile
are not established by these documents.
The review's CI cadence is an orchestration prerequisite, not permission here
to create a schedule. Segment implementation waits for DX-A1 baseline evidence.

## Decision-to-contract matrix

Each row covers the named proposal section, including its alternatives and
transition obligations. D01–D14 refer to proposal §8 in the decision record.

| Proposal section | Decisions and decision 077 disposition | Contract and owner evidence |
|---|---|---|
| §1 product/principles | D01/D02/D04, amended by decision 078: retain pack, map, privacy and authoritative binary state; identity and freshness use the own digest and file metadata only; pure Rust does not mean no dependencies | C01/C02/C04; B0/B1/B3 boundary and privacy corpus; A3 audits resolved closures, including native/unsafe/build/dev dependencies |
| §1 landed gaps | No claim that the IO-heavy core, stale lock, or text defaults already conform. Broad git use in the core is replaced by DX-P1c (landed): the core never reads `.git/` and never spawns git | C11; DX-P1c (landed) replaces DX-P1; DX-P2 and VM lock owner prerequisites remain; D3 removes compatibility IO only after consumers migrate |
| §2 crate DAG/layout/facade | D01 accepted: pure domain → public ports; adapters and modes composed by one facade | C01; [API/ports v1](../../design/contracts/API-PORTS-v1.md); B0/B1/D1 import and isolation tests |
| §2 features/one binary | D02/D13, amended by decision 078: one `dctx` executable accepted; default Rust parser uses `syn`, so zero external crates applies only to pure core/codec. The core never links or spawns git | C02; A3/D1/E6 profile inventories; no native parser in the core; VCS behaviour is plugin-only |
| §2 DTS | D07 accepted: DTS controls every mode, bounded by host permissions | C03; [DTS v2](../../design/contracts/DTS-v2.md); B2 v1 parity and final-output budgets |
| §3 alternatives A–E | D03 accepted: pure-Rust segments target; A SQLite baseline/rollback, B limited shared-fact comparison, E ephemeral fallback, D benchmark reference | C06/C11; C1–C5/E6 correctness and A/B/C quality evidence before default promotion |
| §3 identity/reuse/freshness | D04 superseded by decision 078: own versioned, algorithm-tagged content digest (first-party SHA-256, `crates/dctx-hash`) is the only content identity; freshness, view and revision identity use that digest and file metadata only. The core never uses git object ids | C04; [identity v1](../../design/contracts/IDENTITY-v1.md); B3/B5/C5 file-metadata freshness, same-stamp, transform and contextual reuse tests |
| §3 segments/publication/mmap | D03/D05: immutable segments, safe buffered decoder, sorted dictionary first; mmap/FST remain conditional | C06; [segment/manifest v1](../../design/contracts/SEGMENTS-v1.md); A2/C1/C3 crash, fuzz and lease tests |
| §3 parsing | D06 amended: `syn` Rust; first-party JS/TS/Markdown lexers; optional tree-sitter only | C05; [parser v1](../../design/contracts/PARSERS-v1.md); B4 per-language held-out precision/recall and refusal tests |
| §3 performance/quality | D14: targets are unmeasured; no speed claim from format alone | C10; A1/E6 pinned corpus, latency boundaries, memory/disk and token experiments |
| §4 locations/privacy/lifecycle | D08 amended by decision 078: time, size and automatic cleanup; ownership and expiry use dctx manifests, leases, TTL and file metadata only. Parameters below are Stage-0 defaults | C07; [cache v1](../../design/contracts/CACHE-v1.md); A2/C4 fake-clock/quota/model evidence with no `.git/` read and no git executable |
| §4 outputs/legacy/migration | D09 carried forward: user exports require adoption; pack history is not disposable map data | C08; D2/D3/C4 consent, import, history-reset and quota tests |
| §4 P1–P8 | D14: properties remain release gates, not proved claims | Owner/test table below; A2 model plus B/C/D/E integration evidence |
| §5 Host integration | D01/D10: linked facade through CodeMap; process adapter retained explicitly, never silent fallback | C09; D4/D5 fake/process/library parity and zero-child-process spy; no Node kernel changes |
| §6 public tree/licence | D11 open; D12 facade accepted, Tauri→GPUI sequence is proposed | C01/C11; A3 rights/licence review; E4/E5 API and visual parity |
| §6 bindings/releases/versioning | D13 facade accepted for Rust/CLI/Python/Node/C/desktop; packaging and unsafe FFI exception not granted | C02/C09/C11; independent versions below; E1–E3 conformance, E6 release metadata |
| §7 stages/disposition/rollback | D14 proposed execution plan; preserve useful landed behavior, migrate in bounded steps | C11; A0b supplies contracts, A1/A2/A3 evidence next; B/C/D/E remain future owners |
| §8 decisions 1–14 | Accepted scope and open details recorded individually. DX-ARCH-04 is superseded by decision 078; the digest is first-party SHA-256 in `crates/dctx-hash` | [Decision register](ARCHITECTURE-DECISIONS.md); no implied licence/dependency/release grant |

## Product obligations

**C01 — one implementation.** Pure domain owns IDs, policy, budgets and rules;
mode engines perform deterministic orchestration through injected ports. Only
adapters do IO. `dctx` composes pack/map/templates/cache for CLI, Python, Node,
C ABI, Tauri, GPUI and kernel hosts. Frontends never reimplement selection,
indexing, ranking or retention. Pack and map coexist. Future modes use the same
DTS plan and permissions. Library sessions have explicit source/storage/clock/
cancellation; no home discovery, global logging or stdout. The core reads
`.gitignore` as plain-text ignore rules and does not edit those files.
`.gitignore` edits and `git add` belong only to `dctx-plugin-git`.

**C02 — honest profiles.** Install one executable `dctx`, retaining no-argument
TUI and `dctx .` compatibility. Default product parsers are pure Rust with
`syn` for Rust; this has external Rust dependencies. Pure core, model, DTS rules
and segment codec target std-only and forbid unsafe. A minimal injected-port
facade has no ambient IO. Tree-sitter, SQLite bridge, mmap, SCIP and binding/UI
dependencies are disclosed capabilities. The core never spawns git, never reads
`.git/`, and never links a git implementation. VCS-aware behaviour is a plugin
through the plugin API (`dctx-plugin-git` for `.gitignore` edit and `git add`).
Optional features must not become defaults through Cargo feature unification. No
default subprocess/native parser or claim that all transitive Rust code is safe.

**C03 — organization policy.** DTS v1 output remains compatible; v2 compiles
selection, extraction, budget, view and rendering policy for every mode.
`effective_permission = host_grant ∩ organization_ceiling ∩ session_selection`.
Authorization precedes hashing, parsing, content-cache access and source reads;
`Exclude`, `DebugVisible` and `OmitContent` never authorize file contents.
Unreadable/changed policy invalidates cached authorization. Policy is checked
again before emission, even for an existing pinned snapshot. Metadata visibility
does not expose old summaries or signatures.

**C04 — independent identity.** The source of truth is dctx's own
algorithm-tagged digest of exact authorized bytes (first-party SHA-256,
`crates/dctx-hash`) together with file metadata. Root, view and revision
identity do not use a git object id. Fact keys include language, parser
revision, extraction digest and schema. Resolution is per view/environment.
Freshness is file metadata plus validation of authorized bytes against that
digest. The core never reads `.git/`, never spawns git, and never uses git
object ids. A plugin must not replace this digest or skip it. Identity v1
states the NIST and fuzz differential gates for `crates/dctx-hash`. See identity
v1 for framing and migration.

**C05 — truthful extraction.** Rust `syn` yields syntax, not compiler semantics.
JS/TS/Markdown use bounded hand-written lexers with explicit unsupported scopes.
Parser coverage (`syntax`, `structural`, `opaque`) and completion (`complete`,
`partial`, `refused`) are independent of edge confidence (`exact`, `name`,
`guess`, unresolved). Never claim exact calls
from syntax alone. All source spans identify their bytes and parser provenance.
Unsupported/malformed input returns useful conservative facts plus diagnostics,
or a typed limit/error; optional tree-sitter requires explicit selection.

**C06 — immutable index.** Segments, fact payloads and published manifests are
immutable, versioned and validated. Publish new objects before an atomic
generation pointer; pin readers before GC can retire their generation. Buffer
reads safely; no native struct casts, live truncation or mandatory mmap. Overlay
replacements/additions/tombstones mask base entries and invalidate dependent
resolution. Corruption is a typed rebuild/refusal, never unchecked decoding.

**C07 — bounded cache.** Time expiry retires unpinned manifests; LRU handles
pressure; reachability reclaims only unreferenced data after grace. Account for
all physical managed bytes and outstanding reservations globally, including
legacy coexistence, staging and pending unlink. Opportunistic automatic cleanup
runs on invocation, with explicit GC for more work; no daemon. Ownership and
expiry use dctx manifests, leases, TTL and file metadata only. The core never
reads `.git/` and never uses commit reachability. Pins protect lifetime, never
permit capacity growth past the hard limit.

**C08 — explicit outputs.** Explicit `pack` writes to a caller-selected sink,
stdout/clipboard or output path and bounded managed history. It creates no
unsolicited text export. Keep legacy alias behavior for one announced minor
compatibility release, then align text-output-off at a breaking release. Never
auto-delete source/config/template sources or user exports. Legacy adoption is
explicit and revalidates ownership; unsupported pack history needs an importer.

**C09 — consistent surfaces.** All five map verbs share limits, moved/gone/
ambiguous results and precision/freshness metadata. Final UTF-8 wire bytes,
including envelope/escaping/continuation, obey byte and estimated-token budgets.
Kernel CodeMap keeps host-owned types and fake/process adapters; direct linking
has cooperative cancellation, not hard process isolation. Python/Node link the
facade. C ABI is designed here but deferred until an audited unsafe exception.

**C10 — evidence before performance claims.** A1 pins source/corpus hashes,
compiler, target, storage, worker count and hardware; E6 compares A/B/C on the
proposal's 5,000 files/50 MiB/100k symbols/300k edges corpus and held-out queries.
Record cold/warm lookup, full freshness/startup, parse reuse, RSS, retained and
temporary disk separately. Proposal §3 targets remain targets (cold ≤5 s,
resident metadata-validated p95 ≤30 ms, persistent ≤40 MiB on that corpus).
Promotion needs correctness and material improvement in at least two of query
latency, cross-root disk and refresh cost without material quality loss.
SPEC §11 additionally needs ≥30 accepted packets/route, exploration tokens −50%,
input tokens −25%, pre-edit calls −50%, no acceptance degradation. Timing checks
run separately from the load-independent suite; no measurements are claimed here.

**C11 — controlled transition.** Keep SQLite/tree-sitter as explicit baseline
and rollback until replacements pass. decision 077 defines the successor default;
retaining today's release temporarily does not redefine that acceptance.
decision 078 defines the successor identity: no git in the core. Candidate format
namespaces are separate. Rollback selects a compatible binary
and namespace; never decode new bytes with an old reader. Root Cargo/lockfile
edits have a dedicated authorized owner. Licence and FFI/dependency approvals
are gates on their affected work, not reasons to block these contract documents.

## Independent version axes

| Axis | Contract sketch | Compatibility rule |
|---|---|---|
| Rust facade/source API | [API v1](../../design/contracts/API-PORTS-v1.md) | Planned source contract; implementation semver begins separately; no stable Rust ABI claim |
| Wire API | JSON v2 in API v1; [golden requests](../../design/contracts/GOLDEN-REQUESTS-v1.json) | Explicit negotiation; retain JSON v1 adapter. Revision identity is the own snapshot digest. The core never emits a git object id |
| C ABI | `dctx_v1_*` in API v1 | Explicit ABI version, opaque handles, owned buffers; exception gate |
| DTS source / compiled plan | [DTS v2 / IR v1](../../design/contracts/DTS-v2.md) | Preserve v1 source semantics; unknown required opcodes rejected |
| Content identity / facts | [identity v1 / fact schema v1](../../design/contracts/IDENTITY-v1.md) | Algorithm + identity version are in every key; no digest-length guessing |
| Segments / manifests | [segment v1 / manifest v1](../../design/contracts/SEGMENTS-v1.md) | Independent major/minor and required capabilities; unknown major refused |
| Cache lifecycle ledger | [cache v1](../../design/contracts/CACHE-v1.md) in cache namespace `v2` | Lifecycle contract version is not directory or segment version |

## Numbered properties and implementation owners

Packet names below are proposal owner assignments, not submitted or completed
work. DX-A0b supplies specifications; tests/models must land with their owners.
DX-P1 is replaced by DX-P1c (landed) for the privacy repair in P1.
Every refusal test includes a neighboring successful positive control.

| Property | Lead owner; supporting owners | Required test and falsifying case |
|---|---|---|
| **P1 no unauthorized content**: non-Include ⇒ zero read/hash/parse/cache-content emission | DX-B3; B2/C5/D1 | `p1_selection_precedes_content`: instrument filesystem and cache spies across all decisions, cached allowed→omitted transition and policy-load failure; allowed twin emits; mutation moving auth after lookup fails. Assert the core opens no `.git/` path and spawns no git process. DX-P1c (landed) replaces DX-P1 |
| **P2 immutable visibility**: visible generation references complete validated objects; one generation per handle | DX-C3; A2/C1 | `p2_publish_crash_matrix`: crash at every flush/rename/pointer step with concurrent readers, recover old or new committed generation only; mutation publishes pointer first |
| **P3 safe reclamation**: reachable from retained manifests or active leases ⇒ no unlink/truncate | DX-C4; A2/C3 | `p3_pin_gc_interleavings`: acquire/purge/compact/read/release schedules with live lease and shared roots; collect after release; mutation omits leases from mark set |
| **P4 bounded storage**: committed + staged + reserved + pending ≤ admission cap; legacy oversize admits no growth | DX-C4; A2 | `p4_global_reservations`: two writers, compaction, legacy WAL and failed unlink under fake quota, prove every transition's accounting; mutation drops outstanding reservation |
| **P5 bounded truthful answers**: final wire budget and honest freshness/precision | DX-B2; B4/C5/D1/E1–E3 | `p5_final_envelope`: Unicode/escaping, tiny budget, all five verbs/formats, partial parser and same-stamp stale fixture; no split records or false freshness; mutation budgets records before envelope |
| **P6 deterministic views**: same snapshot/policy/versions/query and frozen clock ⇒ same bytes/order | DX-B5; B2/C1/C5 | `p6_permutation_replay`: vary file/worker/posting order and backend, freeze clock and ranking revision, equal result bytes; mutation tie-breaks by insertion order |
| **P7 correct sharing**: FactKey means one byte/extraction contract; edges use compatible view | DX-B5; B3/B4/C2/C5 | `p7_non_vcs_reuse`: identical authorized files in three roots parse once per retained key but imports resolve per layout; parser/extraction changes and GC force reparse; selection changes reauthorize views. A plugin must not change FactKey. The core never reads a git object id |
| **P8 progress**: finite input, capacity, fair maintenance and eventual lease release ⇒ completion or typed refusal; crash cannot lock forever | DX-C3; A2/C4/D1 | `p8_recovery_progress`: crash writer, cancel work, resume cursor, release pins and exhaust finite work; separate capacity/no-progress refusal case; mutation leaves permanent writer ownership |

DX-A2 models publication, leases, reservations, retirement and recovery for
P2/P3/P4/P8, with weak fairness and explicit finite-state bounds. B2/B3/B5/D1
own policy/plan, source-validation, resolution and cancellation protocol models
for their state transitions; E3 owns the ABI lifetime model if approved. Each
model must check non-vacuous mutations in authorized CI, not just pass happy
traces. Property tests bind these numbered invariants to implementation.

## Engineering and validation boundary

Follow [RUST-STANDARDS](../../standards/RUST-STANDARDS.md) and
[CODE-GUIDELINES](../../standards/CODE-GUIDELINES.md): DDD; public boundaries only;
files about 300 lines; 3–6-line `//!` Rust headers; pure synchronous domain;
typed errors; `forbid(unsafe_code)` and public docs; no production
`unwrap`/`expect`/`panic!`; unit tests at file bottoms, property and integration
tests with positive controls. Approved dependencies only. decision 077 selects `syn`
but does not pin a version or approve an unknown transitive closure; A3/B4 must
record that closure before adding it. No dependency is added in DX-A0b.

DX-A0b's complete declared predicate suite is the two `test -s` commands for
this document and ARCHITECTURE-DECISIONS.md, run once after edits. There is no
remote Cargo command in this packet's `ok` list; no local Cargo or product suite
is authorized here. The full product suite (`cargo test --workspace --exclude
dctx-desktop`) belongs to authorized VM CI/future validation packets, once at
the end of their work. Passing document predicates is not product test evidence.
