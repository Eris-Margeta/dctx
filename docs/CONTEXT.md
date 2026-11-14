<!-- SPDX-License-Identifier: Apache-2.0 -->
# Named context and delivery sessions

`dctx context TARGET` returns a small ranked context page for a file, symbol name,
or stable node ID. Selection, content identities, budgets, deduplication, and both
encodings belong to the engine. Transports forward the engine's page unchanged.
The existing `dctx pack --for TEXT` task-context operation remains available.

```sh
dctx context src/lib.rs --place .
dctx context 's:src/lib.rs#ping' --json --budget 3000
dctx context ping ping --session review-1 --dedupe --json
```

Targets are limited to 1024 UTF-8 bytes without control characters.
A plain file name resolves to its module before symbol search. Symbol names use
the existing engine's ranked `find` results (score descending, stable ID ascending);
use a stable ID to select a particular declaration when names collide. A verified
`read` of the selected target comes first, capped at 40 source lines and the read
verb's existing hard byte/token limits. Then `neighbors` selects at most four
one-hop call/import relations using its existing ranking. `explain` supplies their
signatures and summaries. The engine includes each related node once. There is no
recursive expansion. Files with no call/import relations need only their source
chunk. Read truncation markers and any read continuation are preserved.

The default final-page budget is 3000 estimated tokens; accepted requests are
100..=100000. The estimate is `ceil(2 * encoded_UTF8_bytes / 7)`, including all JSON
escaping, chunk metadata, references, headers, and the final newline. Host,
session, and template policy ceilings also apply. The engine drops related chunks
from the end until the page fits. If the target chunk cannot fit, it returns
`BudgetTooSmall`; it never silently removes the target. `more` counts omitted
related records, including those excluded by the neighbors query's caps. Missing
targets produce an empty page and CLI exit 1. Invalid requests use exit 2.

## Content IDs

Every context chunk has an engine-generated ID:

```text
chunk:v1:sha256:<64 lowercase hexadecimal digits>:<content UTF-8 byte length>
```

SHA-256 covers `dctx-chunk-v1` followed by a NUL byte, then three fields in order:
`role`, `target`, and `content`. Each field is encoded as its UTF-8 byte length
(an unsigned 64-bit little-endian integer), followed by its exact UTF-8 bytes.
The decimal length at the end of the ID measures only `content`.

Roles are `source` and `related`; targets are stable engine node IDs. Source
content starts with `path:first-last` and LF, followed by the verified read text.
Related content contains that location, LF, the signature (or kind), LF, and the
summary. Read locations describe the original indexed node span; truncation is
marked by the existing read renderer. No whitespace or Unicode normalization is
performed. Changed content, role, target, or location changes the ID. Unchanged
chunks retain their IDs across processes, snapshots, budgets, output formats,
and transports. Absolute source roots, timestamps, session IDs, generation IDs,
and transport envelopes do not enter chunk identity.

## Opt-in deduplication

`--dedupe --session ID` enables engine-side delivery history. The first successful
page carries full chunks. Later pages in that delivery session carry the same IDs
with references in place of previously delivered content. Without `--dedupe`,
every page contains full content and does not update delivery history.

A delivery session lasts for one CLI invocation or one RPC connection. Multiple
CLI targets (at most 64 per invocation) are processed in order, producing consecutive text pages or NDJSON
with `--json`. RPC connections can retain multiple independent named sessions.
New connections and new CLI processes start with empty history, even when an ID is
reused. Nothing persists on disk. Clients must retain full chunks while using a
session. Library callers own a `dctx::context::ContextSessions` value and can call
`reset(ID)` to discard history explicitly.

The engine supports up to 128 live session IDs and 4096 unique chunks per session.
Capacity exhaustion refuses the request without eviction. Session IDs must be
1..=128 UTF-8 bytes without control characters. Dedupe still verifies current
source access and freshness; a cached ID cannot bypass a denied or stale read.
Refusals never mark chunks delivered. Selection and budget fitting use full
chunks before reference substitution, so repeated requests select the same context.

## Equivalent surfaces

CLI:

```sh
dctx context ping --json --session review-1 --dedupe
```

RPC uses the existing `context` verb after `hello`, with CLI context arguments.
The older `context` request using `--for TEXT` still selects task context.

```json
{"id":0,"verb":"hello"}
{"id":1,"verb":"context","args":["ping","--session","review-1","--dedupe"]}
```

RPC returns the JSON page as `result.output`. This string matches CLI `--json`
output byte for byte, including LF.
The JSON page has `v`, `verb`, resolved `target`, ranked `chunks`, `more`, and
`next`. Every full chunk has `id`, `role`, `target`, `ref: false`, and `content`.
References have `ref: true` and omit `content`. Text uses `## chunk ID ROLE TARGET`
or `## ref ID ROLE TARGET`, followed by content only for a full chunk.
