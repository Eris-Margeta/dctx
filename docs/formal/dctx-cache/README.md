# DX-A2 cache lifecycle model

Finite TLA+ model of the dctx cache publication and reclamation protocol.
It checks ARCHITECTURE-PROPOSAL §4 properties P2, P3, P4 and P8, and the
registered-root ownership rule from decision 078 (decision 077's git piggy-back is withdrawn).
`check.sh` runs TLC on the pass configs, then `expect-fail/`.

## What one state is

One registered root `r1`, one reader, one old generation, one successor, and
one idle overlay. Byte sizes are units (`Unit = 2`, overlay = 1), not GiB.
`Cap = 5` and `Soft = 4` stand for the global hard cap and the LRU watermark.
`LegacyExtra = 3` is pre-existing oversize: admitted bytes may sit above `Cap`
and must not grow. The logical clock has one tick. It implements idle-overlay
TTL only.

A generation is published by a writer lock, a reservation, a temp segment, a
manifest, validation, then one atomic pointer swap. That swap is the only step
that sets `pointed`. Recovery after a crash (the OS lock is gone; PID age is
not evidence) keeps a swapped pointer and discards unpublished temp bytes. It
does not promote a temp object because its timestamp is newer.

Readers observe one pointer, then confirm a lease only if that generation is
still retained and complete. GC retires an unpinned, non-current manifest under
LRU pressure or TTL, waits one grace step, and unlinks only when the object is
outside the retained manifests, the recovery pointer, and active leases.
Nothing truncates a live size. An explicit pin blocks retirement.

Git object ids are not in `RegisteredRoots`. No pass-config action reads them.

## Properties

| Name | Pass config | Falsified by |
|---|---|---|
| P2 immutable visibility | `CacheLifecycle.cfg` | `expect-fail/PointerFirst.cfg` |
| P3 safe reclamation | `CacheLifecycle.cfg` | `expect-fail/OmitLeases.cfg` |
| P4 bounded storage | `CacheLifecycle.cfg`, `CacheLegacy.cfg` | `expect-fail/DropReservation.cfg` |
| P8 progress | `FairSpec` in the two fair configs | `expect-fail/StuckWriter.cfg` |
| No git ownership | all pass configs | `expect-fail/GitOwnership.cfg` |
| Explicit pin | `CachePinned.cfg` | pin guard removed from `Retire` / `ExpireOverlay` |

P8 is a temporal formula under weak fairness of four actions: the writer-step
disjunction, crash recovery, lease release, and the maintenance disjunction
(drop previous pointer, LRU retire, grace, pending unlink, gone). At each
stage only one disjunct is enabled, so that fairness is the next step.
Fairness does not include `Tick`, `Touch`, `BackwardClock`, `ExpireOverlay`,
`Crash`, or `ReaderBegin`. A run that never advances the clock can still
publish, refuse with `CacheLimit`, recover a crashed writer, and reclaim a
superseded generation by LRU. The `TLC_TIMEOUT` in `tlc.sh` only bounds the
checker.

`CacheLegacy.cfg` is the refusal control: the writer cannot reserve and P8
ends in `CacheLimit`. `CacheLifecycle.cfg` is the publication control.

Witness configs show that publication, crash, collection, and TTL expiry are
reachable. They are expected to violate `NoPublish`, `NoCrash`, `NoCollect`,
and `NoTtl`.

## Bounds

Not modeled: multiple roots, real 2 GiB figures, the 25 ms / 128-object
startup slice, pack history, purge of user exports, and mmap. Those stay with
the DX-C3 and DX-C4 implementation tests. The model is the interleaving and
crash argument those tests have to preserve.

## Run

`sh docs/formal/dctx-cache/check.sh` on the VM (Java 17+, TLC on the classpath
resolved by `tlc.sh`). Pass configs live next to `CacheLifecycle.tla`. Mutants
and witnesses live in `expect-fail/` and are not part of the pass glob.
