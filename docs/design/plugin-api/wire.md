# Framing and session protocol 1.0

PL-C · normative · PLUGINS.md §§3.1–3.4.
Depends on the closed wire/request/result/event schemas and semantics.md.
PL-P supplies the codec; PL-H supplies session state, budgets and receipts.

## Bytes and canonical inputs

A frame is a four-byte unsigned **little-endian** length followed by exactly
that many UTF-8 bytes of one JSON object. Length excludes the prefix. Reject
zero or oversized length before allocation, partial prefix/body at EOF,
non-UTF-8, BOM, duplicate keys, trailing JSON, excessive depth/members and all
non-integer numeric spellings. No newline framing, compression or autodetection.
Split reads/writes are legal and must reconstruct the same message. Each
direction preserves frame order. stdout contains only protocol; drain bounded
stderr independently and never mix it into pack/map stdout.

Before negotiation the cap is 65536 bytes. Thereafter use the selected frame
ceiling, at most 1048576. A base64 chunk decodes to at most 65536 raw bytes,
with canonical RFC 4648 standard alphabet, required padding, no whitespace and
zero unused pad bits. The encoded JSON **including its envelope** must fit the
frame; shorten chunks accordingly. Base64 decoding is bounded before allocation.
For reads, `offset + decoded_length <= size`; `eof` iff this sum equals size.
Never return more than requested. Empty bytes are valid at EOF.

Canonical bytes used for argument/plan digests are UTF-8 JSON with object keys
sorted by ASCII byte order, no whitespace, original array order, decimal integer
spelling, and no BOM/newline. Escape quote/backslash with `\"`/`\\`, control
bytes as lowercase `\u00xx`; emit other Unicode as UTF-8. Do not normalize
Unicode. Keys in these digest inputs are the ASCII schema/config identifiers.
An action's input is `{method,params}` without `params.action_id`. A plugin-plan
digest hashes the entire compiled plan; source selection/extraction digests are
separate identities. Signed manifests/indexes instead use **exact preserved
file bytes**, including their whitespace: never canonicalize them for signing.

## Bootstrap and lifecycle

Host sends exactly one `hello`, with `wire=1`, `h:` ID, supported ranges, APIs,
hooks, extension names and ceilings. Worker echoes its ID in one result/error.
Result contains chosen version, verified identity and required/optional APIs
and extensions. Identity must match the install record, not establish it.
Required APIs must be offered; optional unoffered APIs are unavailable. Selected
extensions are a subset of the host offer. The base v1 extension registry is
empty; fixtures reject any nonempty extension unless context explicitly
registers and negotiates an extension. No required new semantics in a minor.

After hello, host creates an opaque session ID and sends `initialize` with
`v=[1,0]`. This is the worker's first session binding. Its `ready` response
unlocks invocation admission. Initialize supplies validated config, effective
hooks, grant/resource handles, immutable plan digest, epoch, resource ceilings
and liveness policy. Handles supplied here are inert until ready and an admitted
invocation; negotiation/initialization can never serve content or effects.
Then at most one invoke is active. Completed invocation returns the worker to
ready; shutdown revokes handles and ends/reaps the session. A worker response
alone does not establish reaping. Never keep a daemon beyond its host session.

| Method | Sender / permitted state | Successful result definition |
|---|---|---|
| `initialize` | host / negotiated | `results#initialize` (`ready`) |
| `invoke` | host / ready | `results#invoke` |
| `probe` | host / active or cancelling | `results#probe`, exact challenge echo |
| `cancel` | host / active | `results#cancel`, then cancelled event or forced reap |
| `shutdown` | host / ready or cancelling | `results#shutdown`, then EOF/reap |
| `output.read` | plugin / active | `results#output.read` |
| `output.list` | plugin / active | `results#output.list` |
| `file.read` | plugin / active | `results#file.read` |
| `file.stage` | plugin / active | `results#file.stage` |
| `file.replace` | plugin / active | `results#file.replace` |
| `program.run` | plugin / active | `results#program.run` |
| `state.read` | plugin / active | `results#state.read` |
| `state.write` | plugin / active | `results#state.write` |
| `state.reclaim` | plugin / active | `results#state.reclaim` |
| `progress` | plugin event / active | no response |
| `cancelled` | plugin event / cancelling | no response |

Every session request has a sender-owned `h:` or `p:` ID, unique for the whole
session, including completed requests. Responses use the request's ID, exactly
one result/error and no method. The generic response schema deliberately uses
an `anyOf` for result shapes that overlap. The **pending request's method**
selects the exact result schema; validating the union alone is insufficient.
Unknown/duplicate response IDs, wrong direction, wrong state/session/version,
unsolicited events or a response of the wrong result type are InvalidMessage.
An outstanding request may finish out of order, but only once. Broker requests
are implicitly bound to the sole active invocation and may not survive it.

Event envelope sequence starts at 1 and strictly increases per sender/session;
gaps are allowed. Progress payload sequence also starts at 1 and increases per
invocation. Completed is monotone within a phase and <= total when supplied;
unit/total are stable within that phase. Wait references must name a known
live dependency or outstanding broker request. Progress text cannot contain
source data; phase names are opaque bounded codes, not free-form diagnostics.
A phase name alone proves no work. A new verified phase requires corroborating
host-observed work/dependency advancement; invented or repeated phase names
cannot reset suspicion or restore a budget.

## Invocation boundaries

The invocation schema closes each hook's distinct payload. A project/resource
is always an opaque handle, not an OS path. `before-pack-output-written`
contains planned output metadata after authorization, before creation;
`after-pack-output-written` contains committed handles and generation.
`before-index` runs after frozen selection and before content reads; `after-index`
runs only after complete publication. `cache-gc` runs after core GC and sees
global aggregate counts without other roots' identities plus its own state
handles. `command` uses declared alias/verb and a bounded argv, with optional
explicit project context. No shell interpolation or arbitrary Git passthrough.

Initial output pages hold at most 64 entries. `next` is a host-bound cursor
for that immutable invocation/list type, not a path or offset supplied by the
worker. `output.list` may use only that cursor; pages are likewise bounded,
ordered and nonoverlapping. It returns planned or committed entries according
to the active hook; absence of next means end. Pagination never escapes total
metadata/queue/work budgets or refreshes consent. An outside-project planned
entry contains no outside path or source bytes. Outputs are metadata until
`outputs.read` is independently granted and privacy rechecked.

Hooks run once per operation boundary, never per file or recursively for
plugin writes. A worker may request broker actions while host awaits invoke;
both sides drain concurrently with bounded queues. Requests after cancel or
revocation refuse immediately; revoke handles and discard uploads. An exit
code of zero without invoke's terminal response is PluginExited, not success.

## Resource and liveness rules

All ceiling fields are required in manifests, initialize, plans and invokes.
Effective numeric bounds are minima across manifest, host, organization and
user ceilings. Defaults: one active invoke, 16 outstanding broker requests,
65536 diagnostic bytes, 65536 raw chunk bytes and 8388608 aggregate delivered
bytes per invocation. `read_bytes` counts host-to-worker content and staged
upload bytes cumulatively (including repeated delivery), not final file size.
`upload_bytes` bounds live private upload storage; `state_bytes` bounds retained
state; `queue_bytes` bounds each direction including metadata; `actions` counts
admitted broker work; `work_units` is finite caller-admitted work. Memory is
an adapter-enforced worker ceiling. No universal numeric default is invented
for memory, storage, action or work budgets: caller/distribution supplies them.
`frame_bytes` may be lowered after hello, never raised beyond 1 MiB.

Over-limit input produces ResourceLimit and cancellation; never allocate an
unbounded queue to report it. Enforce quotas before reads/uploads/effects.
Resource declarations do not promise an adapter can enforce them; unsupported
required enforcement refuses admission. Default limits are ceilings, not grants.

Liveness: running → suspect → probe → progressing/waiting/stalled → cancel →
reaped. Keep the last two sanitized progress samples, injected receive times,
process identity/status and available CPU/IO deltas, pending dependencies and
budget counters. Unknown activity is unknown, never zero. Useful progress is
an advancing count, completed broker action, new verified phase or resolved
dependency; heartbeats/output spam/CPU alone do not prove advancement.

Use observation cadence to gather evidence, not a total runtime kill. A slow
advancing worker continues within budgets. Probe challenge demonstrates only
responsiveness. A live wait is shown with its dependency; it does not reset a
deadline. Interactive suspicion offers continue/cancel. Unattended policy is
explicit: fail closed on unresolved suspicion, or a bounded last-resort
inactivity observation window, plus cancellation/reap grace. Escalate force
only after process/dependency/probe and cancellation evidence fails; record why.
User cancellation revokes admission immediately. Reap descendants on EOF,
crash and cancellation; report reap_pending if the OS cannot reap. Tests use
fake clocks/traces, never scheduler-speed assertions. Git silence is not a stall.
