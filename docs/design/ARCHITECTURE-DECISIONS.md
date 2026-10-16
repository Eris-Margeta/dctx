# dctx architecture decisions

DX-A0b · decision record revision 1 · 2026-10-16.
Records the disposition of all fourteen DX-ARCH §8 decisions under decision 077.
Depends on the proposal and L1 review; governs successor product contracts.
Used by L1 to cut implementation packets without inventing approvals.

## Source and precedence

Read with [PRODUCT-CONTRACTS](PRODUCT-CONTRACTS.md),
[DX-ARCH](ARCHITECTURE-PROPOSAL.md) and its
[L1 review](ARCHITECTURE-PROPOSAL-L1-REVIEW.md).
decision 077 in the DX-A0b packet states, verbatim:

> We do not want to rely on [git]. people have other versioning systems as well although rarely. git hashes are also in a crisis currently as they are moving to 64 character hashes. I propose we rely on our own hashing methods and piggy-back on git hashes only if they are present and usable.

> Pure-Rust parsers by default. syn for Rust and hand-written lexers for JS, TS and Markdown, with tree-sitter available only as an optional feature.

> i'd have a time-based cache cleaning, a size limit on the system, and an auto cleaner ... it can piggy-back on git if there is git repos ... but we need to have a solution for a default scenario where files might not be in a git repository - our solution must be future proof as everything. we innovate - we don't piggyback as default.

The packet also records the maintainer's acceptance of the structure (pure core, modes,
adapters, one facade for CLI/Python/Node/C ABI/desktop, DTS for every mode), one
binary and the pure-Rust segment index. This record applies that acceptance;
it does not infer approval of every detail in the earlier proposal.

“Accepted” describes an architectural choice, not delivered implementation or
permission to publish. “Carried forward” is an L1-reviewed proposal detail to
test/review, not a separate Eris decision. “Open” requires an explicit decision
before its dependent release/implementation step. Prefixes below number the
original fourteen items; SHA implementation is a subdecision, not a fifteenth.

## Register: all fourteen DX-ARCH decisions

| ID / proposal §8 item | Status and accepted scope | Remaining detail / implementing contract |
|---|---|---|
| **DX-ARCH-01 Product boundaries** | **Accepted**: pure core, mode engines, ports/adapters and one facade across all named surfaces | C01/API v1; extraction must preserve pack/diff/ignore behavior; legacy IO wrappers are temporary |
| **DX-ARCH-02 Zero-dependency promise / distribution** | **Accepted as amended**: one binary, pure-Rust default; **original whole-default zero-external claim superseded** by required `syn` | Core/model/codec remain first-party/std-only targets; A3 must audit actual parser/profile closure. Do not label default product zero-dependency or transitive unsafe-free |
| **DX-ARCH-03 Map storage** | **Accepted**: pure-Rust immutable segment index with shared facts, manifests and overlays | C06/segment v1; SQLite remains baseline/rollback until A1/A2/C/E gates; accepting format direction does not waive recovery/quality evidence |
| **DX-ARCH-04 Git reuse / own hashing** | **Accepted as amended**: own content identity first, algorithm/version tagged; Git is optional proven-equivalent fast path | C04/identity v1. **Open subdecision: SHA-256 implementation approval**; candidate is first-party safe Rust, NIST vectors and fuzz differential against dev-only `sha2` |
| **DX-ARCH-05 mmap/FST** | **Carried forward**: buffered IO + sorted dictionary first | No explicit Eris mmap/unsafe exception. E7 needs ≥20% measured gain in selected metric plus audit/property parity; otherwise defer |
| **DX-ARCH-06 Parsing** | **Accepted as amended**: `syn` Rust default, hand-written JS/TS/Markdown lexers; tree-sitter optional | Supersedes optional-`syn`/structural-Rust default and SPEC §13a mandatory tree-sitter. Parser v1 defines honest precision; A3/B4 pin/review exact dependency closure |
| **DX-ARCH-07 Organization language** | **Accepted**: DTS controls every mode with host privacy ceiling | C03/DTS v2. Typed IR contract is specified; exact new source grammar remains compiler-owner work, with v1 compatibility |
| **DX-ARCH-08 Cache lifecycle** | **Accepted direction**: time-based expiry, size cap, automatic cleaner, all independent of VCS | C07/cache v1 carries proposal's numeric defaults; exact tuning is Stage-0 policy, not a quote from Eris. Git reachability is not a prerequisite or GC root |
| **DX-ARCH-09 Normal outputs** | **Carried forward**: explicit pack outputs and managed retention; legacy exports require consent/adoption | C08/cache v1; announcement/version timing and legacy importer fixtures belong to D2/D3 |
| **DX-ARCH-10 Host integration** | **Accepted facade structure**; linked CodeMap adapter and explicit process fallback **carried forward** | C09/API v1; R-60 remains real; dependency closure and kernel wiring require own authorized packets; zero hidden child processes |
| **DX-ARCH-11 Licensing** | **Open** | ADR/README proprietary desktop claims conflict with existing MIT files. the maintainer decides future terms after A3 rights review; no relicensing or revocation of existing grants here |
| **DX-ARCH-12 Desktop direction** | **Accepted shared facade**; Tauri-now/GPUI-later presentation sequence **carried forward** | E4/E5 parity, platform/licence/dependency review; no new engine or independent cache behavior |
| **DX-ARCH-13 Public releases** | **Accepted one facade** for Rust/CLI/Python/Node/C ABI/desktop and one CLI binary | Versioning/packaging sketches carried forward; C ABI gated on explicit audited unsafe exception; no publish grant, package-name claim or approved binding versions |
| **DX-ARCH-14 Engineering acceptance** | **Carried forward from L1 review**, amended by decision 077 | P1–P8, formal models, baseline and staged ownership; do not treat the proposed ~25 packets as dispatched or the targets as measurements |

## Open decisions with concrete candidate outcomes

**SHA-256 implementation (DX-ARCH-04a).** Own hashing means an independently
defined dctx identity; it does not prove which implementation Eris approved.
The requested candidate is a first-party safe SHA-256 module, streaming and
bounded, implementing standard SHA-256 without cryptographic invention.
It uses no production `sha2`, assembly, native library or unsafe. `sha2` is a
dev-only independent oracle for a fuzzed differential test. Identity v1 fixes
the algorithm contract independent of which implementation eventually passes.
L1's earlier production-`sha2` alternative is not the selected candidate here.
Implementation acceptance stays open until explicit approval and NIST/boundary/
streaming/fuzz evidence; failure blocks promotion of that implementation.
Integrity/deduplication is the use case, not hostile-writer authentication.

**Licence (DX-ARCH-11).** Candidate: preserve MIT for existing libraries, CLI,
bindings and MIT desktop code; consider paid signed builds/support. Newly
authored GPUI presentation terms require rights/dependency review and the maintainer's
choice. Existing MIT grants cannot be revoked by updating a manifest. A3 owns
the inventory; E4/E5/E6 consume the decision. This packet edits no licence.

**Capability approvals.** mmap, C ABI unsafe boundary, PyO3/maturin, napi-rs,
GPUI, SCIP and any native Git implementation remain separately reviewed
capabilities. decision 077 authorizes the parser direction, not an unspecified `syn`
version/license/unsafe closure. Standards require the implementing packet to
name exact versions and obtain missing dependency acceptance. Documentation
sketches require none of these additions and can be completed now.

## Explicit changes to earlier text

| Earlier recommendation | Successor rule |
|---|---|
| Git OID may directly serve as ObjectKey; canonical SHA-256 alias optional | Own algorithm-tagged/versioned digest is canonical; Git OID is an optional mapping to it with byte-equivalence and freshness evidence |
| View identity includes mandatory full commit/tree ID; rev header looks like a Git SHA | Own source manifest/snapshot identity is mandatory; VCS metadata is optional, typed, full length and never a substitute |
| Structural Rust parser in strict default; optional `syn` | Default Rust uses `syn`; minimal port-only build may decline Rust parsing, never silently downgrade requested syntax |
| Tree-sitter adopted in SPEC §13a / mandatory in current map | Optional enhanced parser for successor; legacy release kept only through measured migration |
| Whole default product has zero external production crates | Core/domain/codec can be std-only; default parser includes external Rust crates and must disclose them |
| Commit reachability could drive cache ownership | Explicitly registered dctx manifests + leases drive ownership; TTL and byte accounting work identically with no `.git` or Git executable |
| Broad Git status proves freshness | No broad status, filters or denied blob reads; optional scoped evidence may assist; strict validation uses authorized bytes |
| SHA implementation choice inferred from “own” | First-party safe SHA-256 is the concrete candidate with verification gates; explicit approval still open |

## Delivery order and evidence

L1's immediate DX-P1 privacy and DX-P2 cache repairs precede replacement work;
the stale lockfile needs authorized VM CI evidence. This record does not assert
those prerequisite packets completed. A1 measures the working baseline before
Stage 2; A2 models lifecycle interleavings; A3 resolves dependency/licence facts.

Stage 1 separates pure rules, ports, DTS, source adapters, parsers and resolver.
Stage 2 implements safe codec/store/cache and comparison bridge. Stage 3 exposes
the stable facade, CLI controls and kernel adapter. Stage 4 adds surfaces and
release evidence; E7 optimization follows a measured bottleneck. CLI/facade/
cache value may ship before segment promotion, as L1 recommends.

Contracts assign SHA implementation and source identity to DX-B3 (using a
domain module owned by B0 if needed: recut its ownership before dispatch),
parser contracts to B4, segment codec to C1 and lifecycle to C4. No worker may
infer new filesystem ownership from this document. P1–P8 owner/test mapping is
in PRODUCT-CONTRACTS; versioned sketches and golden requests are in `contracts/`.

No code, runtime behavior, benchmark result or formal proof is delivered here.
The two document existence predicates are the entire DX-A0b declared suite;
product Cargo validation remains an authorized remote CI responsibility.
