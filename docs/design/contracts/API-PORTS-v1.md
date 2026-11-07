<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Facade, ports and wire contract v1

Status: current design reference. One library boundary serves all surfaces. API sketch v1, JSON protocol v2 and proposed C ABI v1 are independent version axes.
See [product contracts](../PRODUCT-CONTRACTS.md) for scope and compatibility.

## Ownership and entry points

```text
SessionBuilder(Source, FactStore, CachePolicy, EffectivePolicy, ContextPlan,
               Clock, Cancellation, OperationLimits)
  -> Result<Session, DctxError>
Session.capabilities() -> Capabilities
Session.snapshot(SnapshotRequest) -> Result<SnapshotHandle, DctxError>
Session.pack(PackRequest, BoundedSink) -> Result<PackReport, DctxError>
Session.query(SnapshotHandle, MapRequest, BoundedSink) -> Result<AnswerMeta, DctxError>
Session.compile_template(DtsSource, CompileLimits) -> Result<ContextPlan, DctxError>
Session.cache_status / cache_gc / cache_purge(explicit typed request) -> CacheReport
```

Convenience `map/find/neighbors/explain/read` methods construct the same
MapRequest. No engine logic lives in wrappers. Session construction never scans
home, edits `.gitignore`, emits stdout or chooses storage implicitly. The core
reads `.gitignore` as plain-text ignore rules. `.gitignore` edits and `git add`
belong only to `dctx-plugin-git` through the plugin API. Ephemeral
is an explicit policy with memory bounds, not a hidden fallback from disk errors.
Facade is synchronous; frontends schedule bounded work. Async runtimes stay in
adapters/frontends. No SQL, tree-sitter nodes, raw storage paths or FFI pointers
appear in domain results. Rust results are owned or borrow a pinned handle.

Handles pin one generation and are namespace/session scoped. Closing the last
handle releases its lease; existing result bytes remain owned by the caller.
Cloning a handle increments an owned lease reference. Concurrent reads require
advertised Send/Sync-safe capabilities; mutable session administration serializes.
Current policy epoch is checked at each content emission: a pin protects disk
lifetime, not perpetual access. A cancellation token may be signaled across
threads; it never grants a forced thread kill.

## Boundary ports

Ports live in `dctx-ports`, depend only on public core/model types and return
typed errors. Engines depend on ports, never concrete adapters. Pure model/core/
codec/compiler have no fs/environment/process/global clock imports.

| Port / sketch operations | Required semantics |
|---|---|
| `Source.inventory(Selection, Limits)` → bounded metadata pages | Confined root-relative entries; no contents for non-Include; no recursion into excluded directories; source/path capability is explicit |
| `Source.open(AuthorizedPath, FreshnessMode)` → CheckedSource | Authorization token binds root, current policy epoch and decision; adapter checks it before opening; immutable snapshot or stated live-read limits |
| `CheckedSource.read(chunk_limit)` / `verify(ObjectKey)` | Bounded exact bytes; source identity/races checked; content read/hash share one stream where possible; `read` spans emitted only after full required identity verification |
| `FactStore.pin(ViewKey)` → SnapshotHandle | Atomic reader/GC acquisition handshake; validated immutable generation; bounded Busy retry |
| `FactStore.get_facts(FactKey, authorization)` / `query(handle, plan)` | Validate namespace, epoch and compatible key before content lookup; no backend types escape |
| `FactStore.begin(view, Reservation)` → Writer | One writer/view; publish validated immutable closure, commit generation or abort; cancellation before commit cannot expose staging |
| `Cache.reserve(bytes, root, purpose)` → Reservation | Global atomic quota across processes, convert unused/materialized buckets exactly; drop/abort reconciles actual bytes |
| `Cache.lease / status / maintain / purge` | Lifecycle v1 ownership, TTL, LRU/reachability, explicit destructive request; query callers have no implicit admin capability |
| `Clock.wall / monotonic / boot_identity` | Injected/frozen for replay; optional boot ID, never PID-age liveness proof |
| `Cancellation.checkpoint()` | Checked at file/chunk/posting-block/resolution-batch boundaries; returns `Cancelled`, preserves committed generation |
| `BoundedSink.reserve_record(encoded_size)` / `write_record` | Final encoding preflight includes framing/escaping/continuation; no partial record on budget refusal; IO failure may still leave transport partial output |

The core has no VCS port. Freshness and revision identity come from `Source`
file metadata and the own digest (`crates/dctx-hash`). The core never reads
`.git/`, never spawns git, and never uses git object ids. A plugin that
implements VCS behaviour uses the plugin API and cannot supply a substitute
identity, freshness proof, cache owner or expiry.

Publication has one commit point. Cancellation before it aborts staging; after
it returns a committed report with cancellation observation rather than claiming
rollback. A post-commit IO failure includes committed generation where known.
Ordinary query errors do not mutate source or reset history.

## Requests, budgets and results

```text
MapRequest = { request_version:1, verb, target_or_query, filters, depth, direction,
               selection, freshness, limits, output_format, continuation? }
PackRequest = { request_version:1, selection, snapshot_or_diff,
                history_policy, output_sink, limits, ContextPlan }
OperationLimits = { encoded_bytes, estimated_tokens, records, depth,
                    source_bytes, parse_bytes, memory_bytes, work_units, deadline }
Answer = { generation, SourceSnapshotDigest, verb, records, more, next,
           freshness, precision, diagnostics, history_reset? }
```

`estimated_tokens = ceil(2 * final_utf8_bytes / 7)` using checked arithmetic.
If token budget is B, byte ceiling from it is `floor(7*B/2)`; intersect explicit
byte cap, mode hard limits and host/DTS/session limits. This is an estimate,
not a tokenizer guarantee. An optional host tokenizer has a distinct capability.
Encoding-specific wrappers count against their own final output budgets; the
same authorized semantic result is used, but JSON overhead may fit fewer whole
records than text. Selection and continuation are deterministic per encoding.

| Verb | Default | Hard cap in addition to final byte/token budget |
|---|---|---|
| `map` | Root outline, depth 1, 500 estimated tokens | Depth 3, 60 records, 2,000 estimated tokens |
| `find` | 10 hits, 500 estimated tokens | 40 hits, 2,000 estimated tokens |
| `neighbors` | 20 edges, depth 1, 500 estimated tokens | 60 edges, depth 2, 2,000 estimated tokens |
| `explain` | 400 estimated tokens | 1,000 estimated tokens, at most 5 in/5 out edges |
| `read` | 120 lines, common 500 estimated tokens | 400 lines with explicit full, still ≤2,000 estimated tokens |

Oversized requested limits return `InvalidRequest` or an explicitly reported
narrowed effective limit; never expand host ceilings. `read --lines` is checked
against the node span and whole-record output; head/tail truncation includes a
precise middle omission marker and continuation. If envelope/refusal itself
cannot fit, return out-of-band `BudgetTooSmall {minimum_bytes}` without violating
the sink bound; a CLI diagnostic goes to separately bounded stderr and nonzero
exit, not oversized stdout JSON.

Public IDs use SPEC path/qualified-name form and collision-safe aliases. Old ID
lookup returns `Moved {old,new}`, `Gone {old}` or `Ambiguous {candidates}`; no
arbitrary match. Empty/refused results provide a reason and one bounded next
action when meaningful. Continuations bind query/selection/generation/ranking
revision/encoding and last sort key; changed inputs yield `StaleContinuation`.
Never accept a cursor as a grant to a path. Record order is score descending,
full public ID ascending; adjacency additionally groups by edge kind.

## JSON v2 and compatibility

Wire v2 is explicit (`--protocol 2` in the future CLI sketch); retain the old
JSON v1 adapter through its announced deprecation. Example shape:

```json
{"v":2,"place":"fixture","snapshot":"<full own snapshot digest>","generation":1,"verb":"find","freshness":{"mode":"immutable_snapshot","state":"verified","age_s":0},"precision":{"coverage":"syntax","completion":"complete","parser":"syn:<reviewed revision>"},"records":[],"more":0,"next":null}
```

Core results carry the own snapshot digest, generation and freshness. They do
not carry a `vcs` object or a git object id. Text headers show own snapshot
identity with a disambiguated display prefix, generation and freshness; never
invent `rev=<git-sha>`. Legacy JSON v1 `rev` is an explicitly tagged own
snapshot representation. A consumer that requires a git object id uses a
plugin. The core returns `UnsupportedCapability` for that request and never
fabricates one.
Keep existing v1 record fields and verb behavior; v2 enriches provenance and
versioning.

Records carry kind/ID/path/line and byte spans, summary/source, parser/precision
overrides and edge confidence where applicable. Canonical wire order follows
the example's envelope order, then record schema order; no insignificant
whitespace, LF terminator. Unknown optional response fields may be ignored;
unknown required versions/capabilities are typed errors. Request field names,
enum tags and limits validate strictly. Do not deserialize unbounded JSON first
and apply limits afterward. Wire golden requests below are future conformance
inputs, not assertions about today's CLI parser.

## Typed error set and capabilities

Minimum stable domain codes: `InvalidRequest`, `PolicyDenied`, `PolicyUnavailable`,
`UnsupportedCapability`, `UnsupportedVersion`, `UnsupportedIdentity`,
`UnsupportedDurability`, `BudgetTooSmall`, `ResourceLimit`, `CacheLimit`, `Busy`,
`Cancelled`, `Stale`, `StaleContinuation`, `CorruptIndex`, `SourceChanged`,
`SourceUnavailable`, `Io`. Include bounded structured details and retryability;
do not leak denied paths/content in messages. Backend errors map at adapters.
Missing/moved/gone/ambiguous IDs are normal query outcomes, not empty success.

Capabilities report exact engine/build/profile, facade/wire/ABI/format/DTS
versions, languages/dialects/parser revisions/coverage, freshness/durability,
source confinement, cancellation granularity, native/unsafe dependency boundary
and subprocess mechanism. The core reports no git executable and no `.git/`
reader. They never equate “Rust” with transitive unsafe-free.

## Binding and host sketches

Python/Node expose the same requests/results/errors and engine capabilities.
Python releases GIL during bounded engine work where valid; Node uses a bounded
worker queue. Cancellation/finalizers close handles once and cannot free data
still in use. Neither binding downloads/spawns CLI or discovers home state.
Tauri/GPUI call the same facade, exposing engine version independently of UI.

Proposed C ABI 1: versioned `dctx_v1_capabilities`, `dctx_v1_session_open`,
`dctx_v1_snapshot_open`, `dctx_v1_query`, `dctx_v1_cancel`, `dctx_v1_close`,
`dctx_v1_buffer_free`. Requests use size/version-tagged structs or explicitly
bounded wire buffers; lengths are size_t with checked conversion. Handles are
opaque registry IDs with generations; stale/closed IDs return `InvalidHandle`.
Output buffers come from dctx and only dctx frees them, once; input buffers
remain caller-owned for the call duration. Close invalidates that ID while
active operations retain internal ownership. Concurrent query/cancel is allowed
only as advertised; no reentrant callbacks in ABI 1. Never unwind across C.
Dangling foreign input/output pointers cannot be validated in safe Rust: unsafe
conversion lives only in a separately audited, explicitly approved FFI boundary.
Absent that exception, defer E3 rather than weakening core `forbid(unsafe_code)`.

Kernel DctxLibrary implements host CodeMap, with explicit source permissions,
cache capability and bounded executor. Preserve the fake and R-60 process
adapter as explicitly selected alternatives. Linked adapter test must observe
zero child processes. The core has no revision adapter that spawns git. Process
isolation remains the option for hard kill deadlines; no silent fallback.
Worker APIs expose five read verbs, not cache/index administration.

## Golden and conformance ownership

[GOLDEN-REQUESTS-v1.json](GOLDEN-REQUESTS-v1.json) fixes one tiny immutable
source corpus whose snapshot digest comes from authorized file bytes and file
metadata only, plus requests for all five verbs and error/identity cases.
It supplies exact expected semantic projections; implementation owners add full
encoded goldens after final wire record layout/ranking constants freeze, without
weakening these expectations. Frozen clock and parser fixture revision remove
environment-dependent timestamps/versions. Store backends, CLI/Rust/bindings and
CodeMap fake/process/library adapters consume the same cases.

D1/E1–E3 test no ambient IO, ownership, cancellation, final wire budgets and
error translation. D4 covers bounded output/timeout parity and no child process.
Every refusal has a positive twin; cross-surface encoding tests include Unicode,
escaping, tiny envelope limits and changed policy epochs. Runtime source/API
semver is independent of this sketch number and does not promise a Rust ABI.
