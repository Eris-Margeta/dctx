# dctx architecture decisions

DX-A0b · decision record revision 1 · 2026-10-16.
DX-A0c applies decision 078 and decision 079. decision 078 supersedes DX-ARCH-04.
Records the disposition of all fourteen DX-ARCH §8 decisions under decision 077,
as amended by decision 078 wherever decision 077 allowed git.
Depends on the proposal and L1 review; governs successor product contracts.
Used by L1 to cut implementation packets without inventing approvals.

## Source and precedence

Read with [PRODUCT-CONTRACTS](PRODUCT-CONTRACTS.md),
[DX-ARCH](ARCHITECTURE-PROPOSAL.md) and its
[L1 review](ARCHITECTURE-PROPOSAL-L1-REVIEW.md).
decision 077 in the DX-A0b packet states, verbatim. decision 078 withdraws every clause
below that piggy-backs on git hashes or git repositories:

> We do not want to rely on [git]. people have other versioning systems as well although rarely. git hashes are also in a crisis currently as they are moving to 64 character hashes. I propose we rely on our own hashing methods and piggy-back on git hashes only if they are present and usable.

> Pure-Rust parsers by default. syn for Rust and hand-written lexers for JS, TS and Markdown, with tree-sitter available only as an optional feature.

> i'd have a time-based cache cleaning, a size limit on the system, and an auto cleaner ... it can piggy-back on git if there is git repos ... but we need to have a solution for a default scenario where files might not be in a git repository - our solution must be future proof as everything. we innovate - we don't piggyback as default.

decision 078 (Eris, 16.10.2026), verbatim, supersedes those piggy-back clauses and
DX-ARCH-04:

> fix privacy leak - let's not use git for anything. it is a dependancy in runtime - we do not want that. CROSS GIT for good. we will not be depending on it.

Content identity, freshness, view and revision identity, cache ownership and
expiry use dctx's own algorithm-tagged digest (first-party SHA-256,
`crates/dctx-hash`) and file metadata only. The core never reads `.git/`,
never spawns git, and never uses git object ids. Any VCS-aware behaviour
belongs to a plugin through the plugin API. The core keeps reading `.gitignore`
files as plain-text ignore rules (privacy).

decision 079: git behaviour (`.gitignore` edit, `git add`) exists only as the
default-shipped plugin `dctx-plugin-git` (`docs/design/PLUGINS.md` once it
lands), never in the core.

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
| **DX-ARCH-04 Git reuse / own hashing** | **Superseded by decision 078**: content identity, freshness, view and revision identity use dctx's own algorithm-tagged digest (first-party SHA-256, `crates/dctx-hash`) and file metadata only. The core never reads `.git/`, never spawns git, and never uses git object ids. Git is not a fast path, evidence source, or metadata source | C04/identity v1. The digest module is `crates/dctx-hash`. NIST vectors and a fuzz differential against dev-only `sha2` remain that module's acceptance tests |
| **DX-ARCH-05 mmap/FST** | **Carried forward**: buffered IO + sorted dictionary first | No explicit Eris mmap/unsafe exception. E7 needs ≥20% measured gain in selected metric plus audit/property parity; otherwise defer |
| **DX-ARCH-06 Parsing** | **Accepted as amended**: `syn` Rust default, hand-written JS/TS/Markdown lexers; tree-sitter optional | Supersedes optional-`syn`/structural-Rust default and SPEC §13a mandatory tree-sitter. Parser v1 defines honest precision; A3/B4 pin/review exact dependency closure |
| **DX-ARCH-07 Organization language** | **Accepted**: DTS controls every mode with host privacy ceiling | C03/DTS v2. Typed IR contract is specified; exact new source grammar remains compiler-owner work, with v1 compatibility |
| **DX-ARCH-08 Cache lifecycle** | **Accepted direction, amended by decision 078**: time-based expiry, size cap, automatic cleaner. Ownership and expiry use dctx manifests, leases, TTL and file metadata only | C07/cache v1 carries proposal's numeric defaults; exact tuning is Stage-0 policy, not a quote from Eris. The core never reads `.git/` and never uses commit reachability |
| **DX-ARCH-09 Normal outputs** | **Carried forward**: explicit pack outputs and managed retention; legacy exports require consent/adoption | C08/cache v1; announcement/version timing and legacy importer fixtures belong to D2/D3 |
| **DX-ARCH-10 Host integration** | **Accepted facade structure**; linked CodeMap adapter and explicit process fallback **carried forward** | C09/API v1; R-60 remains real; dependency closure and kernel wiring require own authorized packets; zero hidden child processes |
| **DX-ARCH-11 Licensing** | **Open** | ADR/README proprietary desktop claims conflict with existing MIT files. the maintainer decides future terms after A3 rights review; no relicensing or revocation of existing grants here |
| **DX-ARCH-12 Desktop direction** | **Accepted shared facade**; Tauri-now/GPUI-later presentation sequence **carried forward** | E4/E5 parity, platform/licence/dependency review; no new engine or independent cache behavior |
| **DX-ARCH-13 Public releases** | **Accepted one facade** for Rust/CLI/Python/Node/C ABI/desktop and one CLI binary | Versioning/packaging sketches carried forward; C ABI gated on explicit audited unsafe exception; no publish grant, package-name claim or approved binding versions |
| **DX-ARCH-14 Engineering acceptance** | **Carried forward from L1 review**, amended by decision 077 and decision 078 | P1–P8, formal models, baseline and staged ownership; do not treat the proposed ~25 packets as dispatched or the targets as measurements |

## Open decisions with concrete candidate outcomes

**SHA-256 implementation (DX-ARCH-04a).** decision 078 defines content identity as
dctx's own algorithm-tagged digest. The module is first-party SHA-256 in
`crates/dctx-hash`: streaming and bounded, standard SHA-256 without
cryptographic invention. It uses no production `sha2`, assembly, native
library, unsafe, or git. `sha2` is a dev-only independent oracle for a fuzzed
differential test. Identity v1 fixes the algorithm contract. L1's earlier
production-`sha2` alternative is not selected. NIST, boundary, streaming and
fuzz evidence in identity v1 govern this module. Integrity/deduplication is
the use case, not hostile-writer authentication. Git hashes are not an input
and not a fallback.

**Licence (DX-ARCH-11).** Candidate: preserve MIT for existing libraries, CLI,
bindings and MIT desktop code; consider paid signed builds/support. Newly
authored GPUI presentation terms require rights/dependency review and the maintainer's
choice. Existing MIT grants cannot be revoked by updating a manifest. A3 owns
the inventory; E4/E5/E6 consume the decision. This packet edits no licence.

**Capability approvals.** mmap, C ABI unsafe boundary, PyO3/maturin, napi-rs,
GPUI and SCIP remain separately reviewed capabilities. A native Git
implementation is not a core capability. VCS-aware behaviour belongs to a
plugin through the plugin API. `dctx-plugin-git` is the default-shipped plugin
for `.gitignore` edits and `git add` only (`docs/design/PLUGINS.md` once it
lands). decision 077 authorizes the parser direction, not an unspecified `syn`
version/license/unsafe closure. Standards require the implementing packet to
name exact versions and obtain missing dependency acceptance. Documentation
sketches require none of these additions and can be completed now.

## Explicit changes to earlier text

| Earlier recommendation | Successor rule |
|---|---|
| Git OID may directly serve as ObjectKey; canonical SHA-256 alias optional | Own algorithm-tagged digest (first-party SHA-256, `crates/dctx-hash`) plus byte length is the only ObjectKey. The core never uses a git object id |
| View identity includes mandatory full commit/tree ID; rev header looks like a Git SHA | Own source manifest/snapshot identity from that digest and file metadata is the only view and revision identity. The core stores no VCS metadata |
| Structural Rust parser in strict default; optional `syn` | Default Rust uses `syn`; minimal port-only build may decline Rust parsing, never silently downgrade requested syntax |
| Tree-sitter adopted in SPEC §13a / mandatory in current map | Optional enhanced parser for successor; legacy release kept only through measured migration |
| Whole default product has zero external production crates | Core/domain/codec can be std-only; default parser includes external Rust crates and must disclose them |
| Commit reachability could drive cache ownership | Explicitly registered dctx manifests and leases drive ownership and expiry; TTL and byte accounting use file metadata. The core never reads `.git/` and never spawns git |
| Broad Git status proves freshness | Freshness is file metadata plus validation of authorized bytes against the own digest. The core runs no git status and reads no `.git/` |
| SHA implementation choice inferred from “own” | decision 078 names first-party SHA-256 in `crates/dctx-hash`. Identity v1 keeps the NIST and fuzz gates. Git is not a hash source |

## Delivery order and evidence

DX-P1 is replaced by DX-P1c (landed). DX-P1c is the privacy repair: the core
never reads `.git/`, never spawns git, and never uses git object ids. DX-P2
cache repairs remain separate work. The stale lockfile needs authorized VM CI
evidence. This record does not assert that DX-P2 or the lockfile prerequisite
completed. A1 measures the working baseline before Stage 2; A2 models lifecycle
interleavings; A3 resolves dependency/licence facts.

Stage 1 separates pure rules, ports, DTS, source adapters, parsers and resolver.
Stage 2 implements safe codec/store/cache and comparison bridge. Stage 3 exposes
the stable facade, CLI controls and kernel adapter. Stage 4 adds surfaces and
release evidence; E7 optimization follows a measured bottleneck. CLI/facade/
cache value may ship before segment promotion, as L1 recommends.

Contracts assign the digest algorithm to `crates/dctx-hash` and source-identity
integration to DX-B3 (using a domain module owned by B0 if needed: recut its
ownership before dispatch), parser contracts to B4, segment codec to C1 and
lifecycle to C4. No worker may infer new filesystem ownership from this
document. P1–P8 owner/test mapping is in PRODUCT-CONTRACTS; versioned sketches
and golden requests are in `contracts/`.

No code, runtime behavior, benchmark result or formal proof is delivered here.
The two document existence predicates are the entire DX-A0b declared suite;
product Cargo validation remains an authorized remote CI responsibility.
