# Cache lifecycle contract v1

DX-A0b · lifecycle/ledger sketch 1.0 · managed directory namespace v2.
Defines decision 077 time/size/automatic cleaning without a VCS dependency.
Depends on identity v1 and segment/manifest v1; used by A2/C3/C4/D2/D3.
All thresholds are Stage-0 policy defaults, not measured tuning results.

## Ownership, locations and configuration

CLI resolves one explicit managed cache root; libraries receive caller-owned
storage or ephemeral storage and never discover a home directory. Defaults:

| Platform | Managed root |
|---|---|
| Linux | `${XDG_CACHE_HOME:-~/.cache}/dctx/v2/` |
| macOS | `~/Library/Caches/dctx/v2/` |
| Windows | `%LOCALAPPDATA%\dctx\Cache\v2\` |

An explicit absolute override wins; legacy `DCTX_MAP_CACHE` is recognized with
a deprecation/effective-path diagnostic. Confine generated paths under the
managed root, disallow source-root overlap and symlink escapes; private user
permissions/ACLs only. Ordinary std adapters assume a trusted same-user
namespace; hostile concurrent path replacement requires stronger opened-source
capabilities or explicit refusal. No cross-user sharing.

Partition `facts/segments/views/roots/pack/templates/tmp/leases` by explicit
trust namespace and format schema. Register only roots opened by a caller;
never enumerate every repository, home directory or VCS history. A managed
ownership manifest records created/adopted artifacts. Source trees, config,
template sources and user-selected exports are not disposable cache state.

## Quotas and expiry

| Parameter | Default / effect |
|---|---|
| Global hard cap H | 2 GiB = 2,147,483,648 bytes |
| Soft watermark S | 1.5 GiB = 1,610,612,736 bytes |
| Eviction target T | floor(1.2 GiB) = 1,288,490,188 bytes |
| Root map view/overlay ceiling | 256 MiB = 268,435,456 bytes |
| Root pack history ceiling | 256 MiB, within H |
| Retained committed views | Latest three per root, subject to TTL/quota and pins |
| Idle overlays | Expire after 7 days without use |
| Root registration | Retire unpinned views after 30 days without use; compact tombstoned registry records later |
| Missing/offline/inaccessible root | On observed failure, protect existing state for 7 days; then ordinary expiry/pressure candidate; explicit pins remain |
| Pack history | Latest snapshot + one predecessor within quota; eviction may reset diff baseline |
| Managed previews | At most two generations, 64 MiB per root, 7 days, within H |
| Incomplete unowned staging | Reclaim after 24 hours; boot change only accelerates proven incomplete/unowned state |
| Unreachable complete object grace | 24 hours after observed retirement; explicit GC may bypass grace under coordinator after proving no leases/recovery references |
| Maintenance cadence/work | Due daily or on pressure/recovery need; startup at most 128 objects and a 25 ms target slice |
| Last-use persistence | Coalesce at most once per root per 5 minutes; no filesystem-atime dependency |

One day is 86,400 seconds. Clock is injected; persist last-use and first-missing
times with ledger revision. Negative/backward clock deltas clamp to zero and
mark a clock diagnostic; do not make state instantly expire. Forward movement
can expire unpinned reconstructible state; active leases/pins always protect
lifetime. Within a process use monotonic elapsed time. Fresh in-memory touches
participate in eviction; persisted coalescing can lose at most its documented
window after crash. Stable tie-break is root/view ID, not iteration order.
Missing-root grace may block eviction and cause capacity refusal; no quota
exception results. Repeated failed probes do not restart the grace clock.

Per-root ceilings limit exclusive view/overlay and history ownership; shared
fact bytes are charged once globally, reported as shared-by-root attribution,
and cannot escape the global cap by appearing in many roots. Caller-supplied
quotas may tighten these values; raising them is explicit configuration.
An object larger than its quota returns `CacheLimit`; no silent auto-expansion.

## Accounting and admission

At any transition define disjoint physical/accounting buckets in bytes:

```text
C = committed objects, including obsolete but retained/pinned and legacy WAL
Sg = materialized staged bytes (not yet committed)
R = granted but not yet materialized reservations
P = logically retired objects awaiting successful unlink
U = C + Sg + P
admitted_total = U + R <= H
admit(new_reservation n) iff U + R + n <= H and applicable root ceilings hold
```

Moving a byte from reservation to stage, stage to committed or committed to
pending never double-counts it. Failed unlink remains P. Directory/ledger/lease
and pointer allocation overhead must be budgeted conservatively, not just fact
payload lengths; adapter reports its accounting allocation unit. Hard cap is
an admission invariant over storage under this manager, not a guarantee against
another same-user process filling the disk. OS disk-full remains a typed error.
On observed external/legacy oversize, freeze growth until safe reduction; only
non-growing reconciliation/reclamation can proceed. A cap lowered below current
use has the same behavior. Emergency metadata capacity is pre-reserved within H.

Single global reservation coordinator covers all roots, processes and legacy
coexistence. Writers reserve before allocation and consume monotonically;
underestimate requires additional admission before more bytes are written.
Cancellation/crash reconciles actual staged bytes before releasing the unused
reservation. Pins are retention constraints, never additional byte allocations.
Compaction/copy migration must reserve peak simultaneous old+new bytes.

At S, retire least-recently-used unpinned owner manifests until target T is
reachable, applying due TTL first. If pins/grace/offline protection prevent T,
report blockers. At H reclaim proven safe garbage before new admission or
return `CacheLimit {needed, available, pinned, pending}`. Explicit ephemeral
mode remains subject to memory/output budgets and is never chosen silently.

## Reachability and lifecycle state machine

```text
Rroots = retained registered-root manifests, overlays, explicit history pins
L = active reader/writer/compaction leases
Live = transitive_closure(Rroots union L union committed_recovery_references)
object states: staged -> published -> retired -> pending_unlink -> absent
lease states: acquiring -> active -> released | owner_proven_dead
```

LRU/TTL selects manifests to retire. Reachability determines which segments/
facts can then be reclaimed. A shared fact referenced by another permitted
root remains live. Git branch reachability is neither required nor sufficient;
optional branch views become roots only through explicit dctx registration/pin.
With no Git installed and no `.git`, every rule above is unchanged.

Segments with >25% dead bytes, or under soft-cap pressure, are candidates for
compaction. Pin input, reserve new bytes, publish replacement manifests, wait
for leases to drain, then unlink old immutable objects. No truncation, including
when optional mmap is used later. If there is no headroom, refuse compaction/
persistent writes rather than deleting live input to make space.

Lease acquisition and GC share the segment contract's short handshake. An OS
lock and session identity (boot identity where available) prove ownership;
PID age/TTL alone never kills an active reader. An ordinary live hung reader
can block reclamation; report it. OS-released locks permit recovery after a
crash. P8 assumes eventual release/fair maintenance; otherwise return bounded
refusal, not a false promise of eventual eviction.

## Automatic maintenance and restart

At each managed CLI/session invocation, inspect the small ledger. When daily
cleanup is due, S is crossed, incompatible state exists or staging is abandoned,
try a nonblocking maintenance lock. If busy, record deferred status and continue
valid reads. Process ≤128 objects and check the 25 ms target between objects;
one filesystem call can exceed the target. Persist a cursor and bounded result.
Capacity admission remains strict even when automatic work is incomplete.

Subsequent invocations resume; explicit `gc` gets caller-selected work/byte/time
limits and cancellation. Idle work requires an explicit operation budget.
No cron, watcher, daemon, launch agent or reboot task is required or created.
After a crash/reboot cleanup occurs at the next invocation, not while dctx is
absent. Validate current/previous committed manifests before reclaiming recovery
inputs. Quarantine corrupt objects, refuse dependent queries or rebuild only
authorized source; do not promote an uncommitted generation by timestamp.

Ledger v1 records policy version/limits, bucket counters, roots/retention,
reservations with owner/session, leases, last/next maintenance, cursor, migration
and pending-delete state. Persist via validated generation + atomic pointer;
checksum/version/capability rules follow manifest framing in segment v1.
Counters are recoverable from owned objects under exclusive reconciliation;
unknown format returns typed refusal, never deletion by filename inference.

## Public administration and legacy safety

```text
dctx cache status [--json] [--root PATH]
dctx cache gc [--dry-run] [--root PATH] [--max-bytes N]
dctx cache purge --root PATH [--dry-run] [--yes]
dctx cache purge --all [--dry-run] [--yes]
```

Same facade operations serve every frontend. `status` reports H/S/T, actual
buckets, live/shared/reclaimable/pinned/pending bytes, per-root attribution,
leases, missing roots, clock diagnostics, migrations, last/next cleanup and
deferred reasons. Dry-run plans are read-only: no access-time update, unlink,
manifest retirement, automatic cleanup or cursor mutation during inspection.
Apply revalidates ownership/leases/capacity; dry-run is not a deletion lock.

`gc` applies configured retention; reports reclaimed versus pending and blocked
reasons. `purge` explicitly selects root/all, drops managed references including
retention pins selected by that request, but active handles keep physical
objects valid. CLI confirms unless `--yes`; API requires a typed explicit
purge request. Shared objects survive other roots' references. Source/config/
template sources/exports never qualify. No claim of forensic SSD erasure.

Legacy `gc --legacy --dry-run` inspects only known registry/configured outputs.
Apply requires explicit item selection/adoption and unchanged identity/ownership
checks. Unknown binary versions, modified exports and unregistered files stay
untouched; extension alone proves nothing. Import old pack history explicitly,
reserve copy space, validate new state and retain old state until legacy readers
release it. Insufficient capacity preserves old state and reports required bytes.
Evicted history sets `history_reset` on the next pack/diff result.

Policy revocation blocks old facts/signatures/summaries before GC, including
future emissions from pinned handles. Physical bytes may remain pinned/shared;
status distinguishes authorization from reclamation. Independent purge/retention
requirements use independent trust namespaces.

A2 models P2/P3/P4/P8 with crash/interleaving mutations. C4 tests every threshold
with fake clocks/quotas and no Git executable/repository, plus backwards clocks,
offline grace, shared roots, full disk, stale owners, active leases, failed
unlink, compaction headroom and dry-run immutability. Add optional Git metadata
and require unchanged eviction results for the same dctx roots/leases/clock.
