# dctx plugins: architecture and API

DX-PLUGIN · 2026-10-16 · Design only; no implementation or release authorization.

Recommendation: **out-of-process plugins using a bounded, versioned JSON protocol over stdio**, with host-brokered effects and enforced confinement. Ship `dctx-plugin-git` as a distinct plugin crate embedded in the default executable and launched in a child process of that executable. Independently installed plugins use the same protocol. The strict core contains neither plugin execution nor Git behavior.

## 1. Requirements

Eris, decision 079 (16.10.2026), verbatim:

> This can be a first of plugins for dctx and it will ship with our app as default. it will be really handy to have dctx edit gitignore and run git add - but with a plugin the concerns are clearly separate. make a plugin architecture plan and a plugin API of course, if needed; and add these back in AS a plugin explicitly stated. cli also needs to be able to see plugins and they need to be easily installable somehow via public github or something.

This design extends [ARCHITECTURE-PROPOSAL §2](ARCHITECTURE-PROPOSAL.md#2-product-layout), incorporating its [L1 review](ARCHITECTURE-PROPOSAL-L1-REVIEW.md), and the subsequent decision 077, decision 078 and decision 079 decisions. decision 078 supersedes the architecture's optional core Git accelerator; decision 079 restores only explicitly granted plugin behavior. [ADR-001](../ADR-001-System-Architecture.md), [ADR-003](../ADR-003-Templating-Engine.md), [ADR-005](../ADR-005-Git-Safety-Strategy.md), [ADR-013](../ADR-013-Monorepo-Structure.md), [ADR-014](../ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md) and [SPEC-MAP-MODE](../SPEC-MAP-MODE.md) supply the existing contracts.

**Source evidence and precedence.** This worktree still imports `dctx_core::git_safety` in `crates/dctx-cli/src/main.rs` and calls it for text output. `git show 85c0f7a:crates/dctx-core/src/git_safety.rs` supplies the historical algorithm (§6). DX-P1b's removal is the required integration baseline, not a claim that it is present in this checkout. No source code changes belong to this packet. The architecture and L1 review predate the later decisions: SHA-256 is now first-party (DX-SHA); licensing is Apache-2.0; core Git acceleration is withdrawn. This proposal does not restore it.

| Requirement | Design consequence / acceptance evidence |
|---|---|
| One binary by default | The default CLI is one executable, including the git plugin's payload and manifest. Its plugin worker is a child invocation of that executable. Installing an independent plugin intentionally adds a managed artifact, not a new required runtime for pack/map. |
| Strict core: zero external production crates, no first-party unsafe | Pure core and protocol types/codecs use safe Rust and std. Process, confinement, HTTPS and signature verification are facade adapters, with separate dependency inventories. Every first-party crate forbids unsafe; no dynamic native loading. |
| Git is optional | Pack, map, hashing, freshness and cache work with no Git installed. Git runs only for an enabled, granted plugin request. No fallback from a missing plugin to the old core implementation. |
| One library contract | CLI, desktop and bindings compose the same facade host. Library sessions discover no plugins, global configuration or programs unless the embedding caller explicitly supplies them (ADR-001 isolation). |
| DTS controls organization behavior | AOT-compiled plugin declarations specify enablement, versions, configuration, ordering and failure policy; templates cannot execute code or expand a grant. This extends DTS, not an alternative organization configuration language. |
| Privacy before content access | Neither plugins nor their subprocesses receive excluded source bytes, historical cache bytes, or hidden paths through diagnostics. Authorization precedes read, hash and delivery; metadata visibility follows the selection decision. |
| Capabilities are scoped | The effective grant intersects manifest requests, user consent, organization ceiling and session selection. Installation, enablement and consent are distinct states. |
| Observable lifecycle | Host-owned logs record every grant, refusal, brokered action and result. Progress and process evidence drive liveness (decision 077); elapsed time alone is not failure. |
| Easy discovery and installation | Public GitHub releases, other HTTPS locations and offline bundles carry a manifest, platform artifact, DX-SHA SHA-256 digest and publisher signature. Search uses a plain JSON index. |
| Default git plugin | `dctx-plugin-git` is shipped and enabled in the default CLI/app, visible in `plugin list`, disableable, and absent from the strict core and strict distribution. ADR-005 local behavior is its responsibility. |

A plugin may consume explicitly selected outputs, propose a named file edit, request an allowlisted external operation, report progress, and implement its own CLI command. It may not import private core modules, override selection decisions or index semantics, discover credentials, bypass consent, recursively invoke hooks, launch a daemon, or obtain arbitrary filesystem/network/process access. An authorized `.gitignore` edit changes rule-file input; only the host can reload it and compute a new selection. Version 1 deliberately excludes parser injection, source transformation, arbitrary shell commands and replacing the authoritative cache store. Future capabilities require a versioned extension and privacy review.

“Shipped and enabled” schedules an eligible hook; it does not override an organization denial or supply consent on behalf of an unattended user. The official installation flow presents the bundled git plugin's narrow grants as part of accepting the default distribution; existing installations need a migration consent record. Without that record, `plugin list` reports `enabled / needs-consent`, and pack still runs. An explicit user disable always persists across upgrades.

## 2. Mechanisms compared

Scores: **2** meets the criterion directly, **1** meets it with a qualification, **0** conflicts. These are architectural judgments, not benchmark results. “Zero dependencies” scores the plugin mechanism's production dependency closure, a stricter test than merely keeping dependencies outside the core. “One binary” means the default CLI distribution; external extensions necessarily add artifacts. Cross-platform scores portability of execution, not proof of a portable sandbox.

| Mechanism | One binary | Memory safety / isolation | Zero dependencies | GitHub installation | Cross-platform | Performance | Total / 12 |
|---|---:|---:|---:|---:|---:|---:|---:|
| Compiled-in safe Rust feature modules, in process | 2 | 1 | 2 | 0 | 2 | 2 | 9 |
| Native dynamic libraries, C ABI / Rust ABI | 0 | 0 | 1 | 2 | 1 | 2 | 6 |
| WebAssembly with an embedded runtime and explicit imports | 2 | 1 | 0 | 2 | 2 | 1 | 8 |
| External processes, versioned stdio | 0 | 2 | 1 | 2 | 2 | 1 | 8 |
| Same-executable workers for bundled plugins + external stdio plugins | 2 | 2 | 1 | 2 | 2 | 1 | **10** |
| Scripts / shell commands discovered on PATH | 0 | 1 | 0 | 1 | 0 | 1 | 3 |
| Remote plugin service over HTTP | 0 | 2 | 0 | 1 | 2 | 0 | 5 |
| Restricted declarative effect recipes only | 2 | 2 | 2 | 2 | 2 | 2 | 12* |

*Recipes score well but cannot implement a general language-agnostic plugin API or independently evolving algorithms. They are useful for DTS configuration, not a complete answer to decision 079. The table is not a mandate to maximize an unweighted total; independent installation, privacy and the strict-core boundary are hard constraints.*

Compiled features preserve one executable and cheap calls, but third parties require rebuilding dctx and share its address space. Safe Rust prevents many memory bugs; it does not isolate an abort, OOM or malicious IO. Dynamic libraries add target/compiler/ABI problems, require an unsafe load/call boundary, and let native faults corrupt the host. Rust has no stable dynamic ABI; choosing C does not solve memory safety.

WebAssembly provides a valuable capability boundary when imports are closed. It adds a runtime and its transitive/native/unsafe audit, resource metering and a second artifact/toolchain format. Runtime size cannot honestly be stated without a pinned engine/profile; expect a material release-size cost, to be measured before adopting it. WASI access must still be restricted. Keep Wasm as a possible future backend using the same logical messages if native confinement proves too costly; do not add it speculatively to the strict build.

External stdio workers are language-agnostic, have a stable explicit wire contract and isolate a plugin crash from the host's address space. std supplies process and pipe IO without first-party unsafe. Process startup, encoding and copying cost more than an in-process call; schedule coarse hooks, never a process per file or symbol. **A process is not a security sandbox.** Enforcing filesystem, network and descendant-process denial requires an OS-specific adapter and a reviewed dependency/platform boundary (§4). That qualification accounts for the dependency score of 1.

The recommended hybrid is still **one process mechanism**, not two plugin APIs. The official binary links `dctx-plugin-git` only in the distribution composition layer. Its private worker entry (`dctx --internal-plugin-worker git`) starts before normal CLI configuration, UI or engines, and speaks exactly the external protocol. It receives only inherited stdio handles and launch identity, not a project path argument. Running that entry manually provides no broker grants. The host validates its own executable provenance; it never runs a relative `dctx` from PATH. A standalone build of the same crate supports independent releases and conformance tests. There is no in-process shortcut for a bundled plugin.

The desktop executable can implement the same early worker entry, avoiding a second bundled helper where platform packaging permits it. If a platform requires a signed helper, disclose that in the app's artifact inventory; a desktop bundle is not the single-file CLI promise. Library embeddings may supply an explicit worker executable or omit plugins; they must never assume their host executable understands dctx's private entry.

Use **length-framed UTF-8 JSON** initially. A compact binary protocol would reduce parsing and wire bytes but adds tooling and schema-evolution friction before a measured bottleneck exists. JSON is easy to inspect and implement in other languages; the strict wire codec can be first-party, safe and bounded. This is an LSP-like request/notification pattern, not an assertion of JSON-RPC or LSP compatibility. A future negotiated binary encoding needs conformance and measured benefit; it must not replace the default by silent detection.

Benchmark cold start, idle resident memory, frame throughput and one git hook end to end, with plugin work and host overhead separately reported on all release targets. Reuse one worker per plugin for a command/session, with at most one invocation in flight per worker in v1. Stop it at session end; no background service or per-user daemon. Resource bounds and cancellation are functional gates; performance measurements belong in an isolated benchmark job, not wall-clock unit assertions.

## 3. The plugin API

### 3.1 Placement and lifecycle

Proposed dependency direction:

```text
CLI / desktop / explicit library caller
  -> dctx facade (operation coordinator; host injection)
     -> pack / map / cache engines -> pure core and ports
     -> dctx-plugin-host -> dctx-plugin-protocol
                        -> process / confinement / audit / effect ports
                        -> selected platform adapters

Bundled worker entry -> dctx-plugin-git -> dctx-plugin-protocol
External executable  <---- framed stdio ----> plugin host
```

`dctx-plugin-protocol` owns pure versioned records, bounded codec and schema validation. `dctx-plugin-host` owns pure admission/lifecycle rules with ports for effects. The facade sequences hooks around existing engine operations; engines do not import the plugin host. No plugin-owned type crosses into core or into the stable map result. Implementation uses the the project code guidelines and the project Rust standards: domain/ports/adapters, typed errors, small files, public documentation, injected clocks and no first-party unsafe. Zero-dependency crates implement their error traits with std rather than adding `thiserror`.

Registry state is `installed -> enabled|disabled`, independently of `verified|quarantined` and `granted|needs-consent|policy-denied|unsupported`. Execution is:

```text
resolve pinned artifact -> verify -> compute grants -> establish confinement
  -> spawn -> negotiate -> initialize -> ready -> invoke -> ready -> shutdown -> reaped
                                            \-> cancel -> cancelled -> reaped
                                            \-> crash/protocol-error -> reaped
```

Confinement is established **before any plugin instruction executes**, including handshake. During negotiate/initialize there are no content or effect grants. A worker cannot gain a grant by stating a different identity in its handshake. Artifact identity, publisher and digest come from the host's verified install record; a mismatch is fatal. Revocation closes admission to broker calls immediately, revokes handles and cancels active work; already delivered bytes cannot be recalled. Reap descendants on EOF, crash or cancellation, with platform enforcement that prevents detaching. No process can outlive its host session intentionally.

| Hook / invocation | Exact boundary | Inputs | Mutation and failure contract |
|---|---|---|---|
| `before-pack-output-written` | After the output plan is authorized, before creating project exports | Planned output roles, project handle, host version; no source bytes | Optional preparation, used by git safety to match the old pre-write placement. A required failure prevents the export; default git failure warns. |
| `after-pack-output-written` | After successful close/publication of named exports | Committed output handles, sizes and selection generation | Read only through granted handles; cannot invalidate or rewrite the pack result. Failure reports `output_committed=true`. |
| `before-index` | After selection is frozen, before any indexing content read | Selection digest, authorized aggregate metadata and index intent | May return a diagnostic or required veto; cannot broaden selection or mutate source. Revalidate policy after hooks. |
| `after-index` | After publication of a complete index generation | Scoped generation handle, authorized counts and freshness | No segment/database path and no raw store mutation. Failure cannot roll back publication. |
| `cache-gc` | After core GC commits its own result | Global aggregate counts without other roots' names, plus this plugin's state handles | Propose reclamation of its own unpinned state only; host checks ownership/leases. Core GC never waits for a plugin to release unrelated core data. |
| `command` | `dctx <alias> ...` or `dctx plugin run <id> -- ...` after routing/admission | Bounded argument vector and optional authorized project context | Only declared command capabilities; same consent, privacy, logging and liveness as hooks. No shell interpolation. |

Default read-only map queries do **not** acquire mutating hook capabilities just because they trigger lazy indexing. `before-index`/`after-index` inherit the caller's effect ceiling; worker/query integrations default to no plugins. Hook results cannot silently alter the five-verb map format, ranking, budgets or confidence. A failed engine operation emits no success hook. Hook scheduling is once per operation boundary, not per output file; multiple outputs are a bounded list with pagination if required. Plugin writes do not recursively schedule hooks.

### 3.2 Wire framing, negotiation and messages

Each frame is `u32 little-endian byte_length` followed by exactly that many UTF-8 bytes containing one JSON object. Initial frame limit is 64 KiB for negotiation. The selected operation limit is at most 1 MiB per frame; host may lower it. Reject zero/oversized lengths before allocation, truncated frames, invalid UTF-8/JSON, duplicate keys, excessive depth (32), excessive members (4,096 across a frame), invalid enums and integers outside `0..2^53-1`. IDs and method names are ASCII strings of at most 128 bytes; arbitrary text is bounded by the frame and its field schema. No floats, NaN, implicit numeric casts, indefinite lengths, compression or newline heuristics. Bytes use base64 in bounded chunks; non-UTF-8 OS paths stay opaque handles, never lossy path conversions.

stdout is protocol-only; stderr is a separately drained, bounded diagnostics channel, never mixed with a CLI's stdout results. The host continuously drains both directions while awaiting a result, so a plugin can ask for an effect during `invoke`. Backpressure uses bounded queues; over-limit senders receive `ResourceLimit` and are cancelled rather than allowed to consume unbounded memory. Unsolicited content on stdout is a protocol failure.

Stable bootstrap envelope (before a version is selected) is `{"wire":1,"kind":...,"id":...,"method":...,"params":...}` for requests. Host sends `hello` with supported `{major,min_minor,max_minor}` ranges, encoding `json`, available hook/API names and resource ceilings. Plugin responds with one compatible `{major,minor}`, its manifest identity and required/optional API capabilities. Select the highest common minor within an accepted major. No common version or missing required capability ends the session with `UnsupportedVersion`/`UnsupportedCapability` before access. Bootstrap changes need a new entry protocol, not heuristic parsing.

After negotiation all messages contain `v:[major,minor]` and an opaque `session`. Sender-owned request IDs use `h:` or `p:` prefixes and are unique for the session. Responses echo the ID; events have a strictly increasing sender sequence. A response contains **exactly one** of `result` and `error`. Events have no response ID. Unknown envelope fields are rejected except a bounded `extensions` object whose keys must have been negotiated. New mandatory semantics require a major bump; minor additions are optional and negotiated. Manifest schema, plugin semver, hook payload version and wire version are independent; compatibility is never inferred from matching release versions.

Host invocation example (IDs and digests below are illustrative):

```json
{
  "v": [1, 0], "session": "s:42", "kind": "request", "id": "h:7",
  "method": "invoke",
  "params": {
    "invocation": "op:19", "hook": "after-pack-output-written", "hook_version": 1,
    "project": "project:1", "policy_epoch": 4,
    "selection_digest": "sha256:<64 hex digits>", "host_version": "0.26.0",
    "outputs": [{"handle": "out:1", "role": "pack-text", "bytes": 8192}],
    "grants": ["grant:outputs-read"],
    "limits": {"frame_bytes": 65536, "read_bytes": 131072, "actions": 8},
    "config": {}
  }
}
```

`initialize` precedes this with validated configuration, effective hooks, handles/grants, cancellation/progress support and session budgets. It returns `ready`; a plugin may refuse an unsupported configuration but may not reinterpret it. Invocation inputs are immutable snapshots; an output handle binds plugin, invocation, policy epoch and generation. Exact limits are the minimum of host, organization, user and manifest ceilings. Initial defaults: one active invocation, 16 outstanding broker requests, 64 KiB diagnostics ring, 64 KiB content chunks and 8 MiB aggregate delivered bytes per invocation. Plugins needing larger outputs must declare and obtain a larger bounded allowance. Streaming does not evade the aggregate limit.

Plugin result:

```json
{
  "v": [1, 0], "session": "s:42", "kind": "response", "id": "h:7",
  "result": {
    "status": "ok", "reason": "completed",
    "receipts": ["receipt:12"], "diagnostics": [], "command_output": []
  }
}
```

`status` is `ok|skipped|warning`; `reason` is a declared stable code. Diagnostics are `{code,severity,resource_handle?}`; the host renders known text, escapes controls and suppresses sensitive paths. `command_output` contains bounded typed records/text for an explicitly invoked subcommand, validated against the approved output schema. It is not emitted by ordinary hooks. Receipt IDs are host-issued, not proof supplied by the plugin. Host derives the actual committed effects from its ledger regardless of a plugin's claimed status.

An error has `{code,phase,retryable,effect_state,receipt_ids,details}`. `effect_state` is `none|committed|partial|unknown`; details are typed safe fields, not a raw stack trace. Stable v1 codes include `Denied`, `ConsentRequired`, `UnsupportedVersion`, `UnsupportedCapability`, `UnsupportedPlatform`, `InvalidConfig`, `InvalidMessage`, `ResourceLimit`, `Conflict`, `Busy`, `Cancelled`, `Stalled`, `PluginExited`, `ExternalProgramMissing`, `ExternalProgramFailed`, `IntegrityFailure`, `AuditUnavailable` and `EffectUnknown`. No automatic retry of a mutating invocation with `partial`/`unknown`. Required hooks stop the parent operation if still reversible; post-commit failures return the committed operation result plus structured failure. Advisory hooks, including default git safety, produce a visible warning and preserve successful pack/index output. Explicit plugin commands return nonzero on failure. A successful process exit without a terminal protocol response is not success.

### 3.3 Broker API and replay rules

| Plugin request | Host contract |
|---|---|
| `output.read {handle,offset,length}` | Recheck current policy and provenance; return authorized bytes only, within byte budgets. Pin while reading; refuse revoked/stale generations. |
| `file.read {resource,offset,length,precondition?}` | Named file capability only, with independent read permission and `Include` selection. Return bounded chunks, size and a host precondition token; later chunks must bind that token and refuse changed files. No directory traversal API. |
| `file.stage {resource,precondition,upload?,offset,bytes,final}` | Stage bounded replacement chunks in host-private temporary storage under the file-write grant. Bind the upload to this resource, invocation and precondition; enforce total size/growth and provenance limits. No project file changes yet. |
| `file.replace {resource,precondition,upload,action_id}` | Authorized atomic compare-and-replace from a completed immutable upload; no create unless separately granted. Preserve metadata where supported, refuse symlink/hardlink/reparse-point races. Return receipt and new token; reclaim the temporary upload. |
| `program.run {operation,args,action_id}` | Execute a registered operation recipe: fixed executable identity, argument schema, working-root binding, scrubbed environment, allowed administrative resources and bounded output. Never arbitrary command text or a shell. |
| `state.read/write/reclaim {handle,...}` | Plugin's own host-owned namespace and quota only. Host mediates reads/writes/deletes and records them. No automatic persistence of source content. |
| `progress {invocation,seq,phase,completed,total?,unit,wait?}` (event) | Validated bounded progress; completed is monotone within a phase. A wait names a host-known dependency or broker request. No source bytes in progress messages. |
| `cancelled {invocation,last_receipt?}` (event) | Acknowledge cancellation and stop requesting work. Host still checks/reaps the process tree. |

Host methods additionally include `probe` (challenge with latest progress/dependency state), `cancel` (reason and invocation), and `shutdown`. `probe` replies prove responsiveness only; they do not prove useful progress. `shutdown` ends a session and revokes all ephemeral handles. File/output chunks must fit the negotiated frame **after** base64/JSON encoding; 64 KiB is a raw-chunk ceiling, not a promise to fit 64 KiB into a 64 KiB frame. Temporary uploads count against private storage, delivered-byte and request budgets, are logged, and are discarded on cancellation/revocation. This allows a 1 MiB `.gitignore` without exceeding a frame. Version 1 exposes no network broker method: `network=[]` is mandatory. Future host/port-scoped network capabilities require a negotiated extension, separate consent and egress review; installer HTTPS is a different host operation.

Each effect has a host-bound key `(artifact_digest, project_identity, invocation, action_id)`. Repeating an identical key/argument digest returns its existing receipt without repeating the effect; reusing the key with different arguments is refused. The host logs intent before execution and result after execution. A crash between the effect and receipt leaves `unknown`, never assumed success or a safe retry. Recovery reconciles a file's checked identity/digest or asks for explicit retry; no exactly-once claim across arbitrary program crashes. A new pack operation is a new invocation; §6's content idempotency prevents duplicate rules across those invocations. Plugin configuration/state migrations cannot run arbitrary installation scripts.

### 3.4 Liveness under decision 077

Use the decision 077 progress-feed rule, replacing the architecture's earlier elapsed-deadline defaults. The named `LIVENESS-NOT-TIMERS` spec was not present among the inspected host project documentation files; this section derives from the recorded decision and requires conformance with that spec when available. Do not invent a fixed “30 seconds per plugin” contract.

For each invocation retain the last two sanitized progress events, receive timestamps from an injected monotonic clock, process identity/status, CPU/IO deltas when the OS provides them, pending broker request and byte/action counters. Unknown activity is `unknown`, not zero. Define useful progress as an advancing completed counter, a completed broker action, a new verified phase or resolved wait dependency. Output spam, repeated phases and heartbeats alone cannot keep an operation healthy forever.

Lifecycle observation is `running -> suspect -> probe -> progressing|waiting|stalled -> cancel -> reaped`. Sampling cadence is configurable and uses time to observe, not to decide total runtime. A lack of progress causes a probe; process exit, broken pipes, dead dependencies, a repeated work-state cycle or exhausted byte/action/memory quotas supplies stronger evidence. CPU activity without useful progress remains suspect and subject to work budgets, not proof of success. A responsive wait on a live broker operation is displayed with its dependency; it does not restart an elapsed-time timer. Users can cancel at any time.

Long-running work that advances within its resource budget continues. When evidence is inconclusive, report `suspect` and the last known activity; interactive clients offer continue/cancel. Unattended callers must provide a liveness policy, including a bounded last-resort inactivity observation window and cancellation/reaping grace, or choose fail-closed on unresolved suspicion. Only after probe/dependency/process evidence and cancellation fail may that policy escalate to forced termination of the process tree. Log why the last resort was used. There is no default total-runtime kill and no guarantee of termination for a progressing infinite computation without a caller's finite work budget. Termination correctness assumes the OS can reap the child; `reap_pending` is visible if it cannot.

Tests use fake clocks, progress traces and fake process observations: slow but advancing work survives; a silent dead worker, replayed heartbeat, output flood, dead dependency and cancellation-refusing descendant are handled; no test relies on host scheduling speed. For the non-instrumented `git` child, broker progress covers spawn/exit and OS activity; absence of Git stdout alone is never evidence of a stall.

### 3.5 DTS organization configuration and command routing

Proposed v2 DTS syntax, **not existing parser syntax**:

```ini
[PLUGINS:engineering]
enabled = ["dctx/git"]
order = ["dctx/git"]
network = "none"

[PLUGIN:dctx/git]
version = "1.0.0"
hooks = ["before-pack-output-written"]
required = false
allow = ["file.read:.gitignore", "file.write:.gitignore", "program:git.add-ignore", "resource:git-admin:stage-ignore"]
config.stage = true
config.create_missing = false
config.marker = "host-version"
before = []
after = []
```

The signed manifest defines bounded config fields (bool, bounded integer, enum, bounded string/list); no arbitrary JSON-schema execution, environment interpolation or executable paths in DTS. Compile with `dctx template build` into the existing versioned `ContextPlan` extension. Validate known plugin IDs, resolved versions/digests, supported hooks, fields, capabilities, duplicate aliases, and cycles. Emit a plugin-plan digest distinct from source-selection/extraction digests: changing a post-pack plugin must not force a map reparse. Missing optional plugins compile to an explicit unavailable entry; required missing plugins fail. V1 DTS remains valid with the distribution default plugin plan, filtered through user grants; libraries still default to no plugin host.

Organization policy is a ceiling. User enable/disable state and project DTS can narrow it; a project file cannot silently approve a new executable or new capability. Config precedence is compiled defaults, explicit user settings, then project/organization plan, within immutable deny/consent ceilings; a user disable is sticky. Resolve version ranges at install/update time into a host-owned lock record; running a project never fetches a plugin or changes its selected version. Built-ins resolve to their containing release digest. Every invocation uses one immutable effective plan; an organization change cancels/re-admits affected invocations.

Within each hook, merge explicit `order` edges and per-plugin `before`/`after` edges, reject cycles and unknown required references, then topologically sort with canonical ID as the tie-breaker. Execute sequentially in v1; do not promise parallel file effects. A failed required predecessor skips its dependents. Independent advisory failures do not skip other hooks. Earlier committed plugin effects remain in the result/receipts if a later required hook fails; hooks are not a cross-plugin transaction. Order affects hooks only, not source selection. The git plugin's preparation writes are followed by selection-policy revalidation before pack begins reading content.

Reserve `plugin`, `pack`, `map`, `cache`, `template`, `help`, `version` and internal worker switches. Built-in commands win; conflicting plugin aliases are refused at enablement, never silently shadowed. `dctx git ...` is an alias for the `dctx/git` plugin, not a Git passthrough. Canonical fallback `dctx plugin run dctx/git -- status` always disambiguates. Preserve `dctx .` and path-based snapshot invocation; use `dctx pack <path>` or an explicit `./path` when a bare directory name equals a plugin alias. Help lists aliases from verified manifests without starting plugins. Unknown commands do not cause a PATH search or download.

## 4. Capabilities and privacy

### 4.1 Manifest and grant calculation

Illustrative release manifest (the artifact digest is a placeholder; the schema packet supplies complete valid fixtures):

```json
{
  "manifest_version": 1,
  "id": "dctx/git", "name": "dctx-plugin-git", "version": "1.0.0",
  "publisher_key": "dctx-release-key-1", "license": "Apache-2.0",
  "protocol": [{"major": 1, "min_minor": 0, "max_minor": 0}],
  "host_api": {"major": 1, "min_minor": 0},
  "hooks": [{"name": "before-pack-output-written", "version": 1}],
  "commands": [{"alias": "git", "verbs": ["status", "protect"]}],
  "capabilities": {
    "required": [
      {"kind": "file.read", "scope": "project", "path": ".gitignore", "max_bytes": 1048576},
      {"kind": "file.write", "scope": "project", "path": ".gitignore", "create": false,
       "max_growth_bytes": 4096}
    ],
    "optional": [
      {"kind": "program.run", "program": "git", "operation": "git.add-ignore",
       "resources": ["git-admin:stage-ignore"], "max_output_bytes": 65536}
    ],
    "outputs": [], "network": []
  },
  "config": {
    "stage": {"type": "bool", "default": true},
    "create_missing": {"type": "enum", "values": [false], "default": false},
    "marker": {"type": "enum", "values": ["host-version"], "default": "host-version"}
  },
  "artifacts": [
    {"target": "aarch64-apple-darwin", "file": "dctx-plugin-git",
     "bytes": 1000000, "sha256": "<64 hex digits>", "format": "native-executable"}
  ]
}
```

Other plugins can declare `outputs.read` scoped by role, operation and byte limit; the git plugin needs output **metadata**, never output contents. All capabilities have typed parameters; a string such as `spawn git` is a display label, not a sufficient grant. `git-admin:stage-ignore` is a bounded recipe resource (§6), not `.git/**`. Unsupported required capabilities block launch; denied optional capabilities are omitted and must yield a truthful degraded result. Configuration `stage=true` with no program grant produces `warning: staging-denied`, not a claim that staging succeeded.

Let `M` be the manifest's requested operations, `U` the user's consent, `O` the compiled organization ceiling, `H` the embedding host/caller ceiling, `S(t)` the current session selection/provenance constraints and `A` the enforceable adapter operations:

```text
G(t) = M ∩ U ∩ O ∩ H ∩ S(t) ∩ A
Allow(action,t) = action ∈ G(t) ∧ current_policy_epoch ∧ remaining_budget
                 ∧ verified_artifact ∧ durable_audit_intent
```

Intersection means semantic containment of scopes, argument sets and paths, with the minimum of numeric quotas; it is not string equality. Explicit denies win. A write grant never implies read, creation, deletion or permission to spawn. A program grant never implies its children, network, arbitrary arguments or environment inheritance. Paths bind to host-opened root/resource identities; no prefix-based containment, `..`, absolute path, symlink, hardlink, Windows reparse/alternate-stream or case-folding escape.

Consent records bind publisher identity, plugin ID, artifact/manifest digest, scope (one project or explicit organization/user-wide), capabilities, limits and policy revision. A pre-consented update policy may authorize compatible versions from the same pinned signer with an equal/narrower capability set, but each installed digest gets a concrete derived record. Publisher changes, scope expansion or a newly required capability suspend execution until consent. Neither a remote index nor a repository's DTS can write that record. Disable revokes admission and stops active invocations; enabling does not fetch or approve anything implicitly.

### 4.2 Enforcement and the native-process limitation

With ordinary `std::process::Command`, a native plugin can read the whole user account, connect to the network or spawn descendants. Removing environment variables and hiding path names do not prevent it. A manifest, code signature or promise to use the broker cannot enforce ADR-014 or make the host's audit complete. **There is no “trusted plugin, run unrestricted” fallback in the privacy-preserving profile, including for the bundled plugin.**

The execution adapter must start a confined worker with no project/home/cache mounts, no network, no ability to launch another program, no inherited secrets/descriptors, and only immutable executable/runtime resources and the protocol channels. Persistent state and effects go through the broker; optional anonymous scratch is bounded, private, nonpersistent and cannot expose project data. Plugin-local computation is not an external action; every externally observable effect must have a broker receipt. The OS boundary must deny unbrokered effects; denied escape attempts terminate/quarantine the invocation and generate host audit events. No raw project root is passed as cwd. Supported high-level runtime resources must be declared in the platform artifact; no on-demand interpreter/package installation.

On Linux, macOS and Windows this requires separate research and conformance for OS isolation facilities, child containment, file-handle confinement and network denial. The design does not assume that a generic Unix sandbox command or Windows process job alone provides those properties. A `ConfinementProvider` reports tested capabilities and establishes the boundary before exec; a platform with no proven provider returns `UnsupportedPlatform`. std-only builds may have the protocol and installation inspection but no native plugin execution. Safe wrapper dependencies/platform helpers belong to optional facade adapters and must publish their unsafe/native inventory; first-party unsafe remains forbidden. If a platform requires first-party unsafe to achieve this, defer that provider or reconsider Wasm, not the safety rule.

The **default plugin-enabled distribution cannot be promoted for a target until its provider passes**. One-binary packaging, a useful git plugin and enforced native confinement are jointly release gates, not three benefits assumed from stdio alone. This is the largest implementation risk. §7 puts the platform feasibility work ahead of host deployment; §8 asks Eris to approve this release boundary explicitly.

The host's file broker also needs race-safe directory/resource handles and atomic replacement. Canonicalization followed by an ordinary path open is not a hostile-filesystem guarantee. Use a reviewed safe platform adapter, caller-supplied protected namespace or refuse. Mutations check object identity at both admission and commit; concurrent policy/resource changes invalidate the request. Brokered programs receive their own narrower confinement profile; granting the plugin worker's profile does not automatically grant one to Git.

### 4.3 Selection, cached data and administrative resources

`Exclude`, `OmitContent` and `DebugVisible` never authorize source content access. Debug visibility supplies metadata only. The broker checks the effective decision before opening, hashing or serving a cached object. It filters both metadata and content responses to the plugin's granted scope; no raw root inventory, global cache path or historical index is exposed. Selected output handles carry the provenance set and selection epoch of their source inputs. If current policy denies any constituent, refuse the entire output or regenerate an authorized projection; never serve stale bytes merely because a handle used to be valid. Derived summaries, plugin state and cross-root references obey the same rule. Stateful plugins may store content only with explicit provenance-tagged storage permission; v1 git state contains receipts/digests, no source bytes.

`.gitignore` read permission is checked separately from write permission. If the current selection excludes/omits its content, the git plugin cannot read it, compute a digest, rewrite it or stage it; return `Denied` and leave it untouched. The user can explicitly revise their selection and consent, but a plugin cannot do so. Host rule-file loading is not a capability to disclose rule-file contents to a plugin.

Git itself necessarily reads and writes repository administration, including its index and a blob for the staged `.gitignore`. Treat this as an explicitly named **administrative effect**, whose internal metadata stays in the broker/Git adapter; the worker never receives `.git` contents. It is not a grant to inspect excluded source, read historical blobs, traverse object packs, or disclose paths stored in the Git index. The recommended `git-admin:stage-ignore` exception is narrowly enumerated and separately visible in consent. If an organization forbids all reads of `.git` including operational metadata, staging is unavailable. This operational exception must be approved in §8; do not relabel excluded source content as “metadata” to evade ADR-014. Where normal Git cannot perform the operation without an excluded read, confinement denies it and staging fails visibly.

### 4.4 Host-owned audit

Each event records host sequence/time, operation/session/invocation IDs, plugin ID/version/artifact digest, policy/plan digest, grant or refusal reason, hook/method, resource handle, action argument digest, byte counts, status, and receipt link. Emit install/verify/enable/disable/update, effective grants, attempted broker actions (including refusals), committed results, progress-state transitions, cancellation and cleanup events to dctx's own local log. `plugin info --json` exposes effective grants and the latest receipt references. A plugin-written success line cannot satisfy the audit requirement.

Log stable safe metadata, not source bytes, file replacement bodies, raw command arguments, environment values, credential-bearing URLs, plugin stderr or command stdout. Digest/handle references are private local metadata too: owner-only permissions and organization separation apply. Arbitrary stderr is drained into a bounded transient channel; persisted events record its size and safe classification, not its contents. An explicit diagnostic export is separately authorized. This both prevents log-based exfiltration of selected content and avoids repeating ignored paths. Plugin command output uses the user's bounded result channel, not the audit log.

Durably append intent before a mutating action and completion after it. If logging is unavailable/full, refuse new effects with `AuditUnavailable`; if completion cannot be recorded after a mutation, retain/reconcile its pending intent and return `EffectUnknown`. Do not report rollback without evidence. Read grants/delivery and refusals are also recorded before data is delivered. Rotate only completed audit segments under a configured retention/space policy; unresolved intents are pinned. A local append log is auditable, not tamper-proof against the account owner. No telemetry upload is implied.

## 5. Distribution

### 5.1 Commands and visible states

All commands are facade operations shared with the app, with bounded `--json` equivalents. Plugin management is routed before legacy snapshot side effects in `crates/dctx-cli/src/main.rs`; listing or searching never creates a project's `.dctx/` or starts Git.

| Command | Contract |
|---|---|
| `dctx plugin search [query] [--refresh]` | Search the cached verified discovery index. Explicit refresh fetches its public JSON and detached signature. Report cache age and trust status; never run code from results. |
| `dctx plugin install <owner/repo>[@version]` | Resolve an exact compatible release, display provenance/grants, download and verify, then install inactive. Without a version, select the newest compatible stable release from verified metadata and pin it; never follow a moving branch at runtime. |
| `dctx plugin install --url <https-manifest-url>` | Same flow from another public HTTPS host. The signed manifest resolves relative artifact URLs; enforce download-origin policy on redirects. |
| `dctx plugin install --file <bundle-path>` | Entirely offline install of a signed local bundle with the same validation and publisher trust gates. |
| `dctx plugin list [--json]` | Installed/bundled ID, version, origin, alias, enabled/disabled, granted/needs-consent, verification, compatibility and platform status. No plugin execution. |
| `dctx plugin info <id> [--json]` | Signed metadata, signer fingerprint, source URL, exact artifact digest, hooks, requested/effective grants, policy reasons, lock record and latest receipts. |
| `dctx plugin enable <id> [--project <path>]` | Select scope, show effective plan and record consent if needed; then enable subject to organization policy. Noninteractive use requires explicit capability approval or an existing scoped consent record. |
| `dctx plugin disable <id> [--project <path>]` | Persist disable, revoke admission, cancel/reap active workers. Does not undo already committed effects. |
| `dctx plugin remove <id>` | Disable, wait for leases/reaping, remove owned inactive versions and selection records. Keep audit and user-authored DTS. Bundled removal masks the plugin; it cannot remove bytes from the executable. |
| `dctx plugin update [<id>] [--version <version>] [--check]` | Check/download/verify candidate versions under a user-requested update policy. Reject capability expansion without new consent. Keep old activation until replacement is complete. No automatic update during pack/map. |
| `dctx plugin verify [<id>] [--refresh-trust]` | Recheck artifact size/hash/signature, signer trust, target/protocol compatibility, revocation freshness and installed-file identity. Quarantine failures. No plugin execution. |
| `dctx plugin run <id> -- <args...>` | Explicit alias-free subcommand dispatch under the normal plugin execution contract. |

For a newly installed third-party plugin the normal path is `install`, `info`, `enable`, then its alias or hooks. `install --enable` can combine those steps with a concrete consent display; it cannot hide grants behind generic `--yes`. Existing consent may cover a verified equal/narrower update. Unknown publishers require a pinned key approval as well as capability approval. Commands return typed errors and nonzero status for refused installs/updates; discovery cannot change execution state.

Reserve canonical `publisher/name` IDs and aliases in the signed registry. A GitHub `owner/repo` is a source locator, not the identity or trust root; require its manifest's claimed ID to match the selected registry entry or the identity the user explicitly approved. Renamed/transferred repositories cannot take over an installed identity without the pinned signing key. Disable stays authoritative if a new bundled release reintroduces the same ID.

### 5.2 Release artifacts, hashes and signatures

Each plugin release publishes:

```text
plugin.json                          # bounded manifest with per-target sizes and SHA-256
plugin.json.sig                      # detached publisher signature over exact manifest bytes
SHA256SUMS                           # convenient generated mirror, must match signed manifest
dctx-plugin-<name>-<version>-<target> # native binary (.exe on Windows)
LICENSE / NOTICE / provenance.json   # licences, source revision, build inputs / dependency inventory
<name>-<version>-<target>.dctx-plugin # optional self-contained offline bundle
```

Start with macOS arm64/x86_64, Linux x86_64/aarch64 with explicit libc/minimum-OS metadata, and Windows x86_64; advertise only targets that pass confinement and conformance. No emulation, build-from-source, post-install script or host interpreter download as a fallback. Other languages are welcome if their release artifact carries its permitted runtime resources and honestly declares them. A language-agnostic wire protocol does not imply a Python source file is a self-contained executable.

Use **DX-SHA's first-party SHA-256** to compare downloaded artifact bytes against the digest authenticated by the signed manifest. Require its NIST vectors, incremental/chunk boundary tests and dev-only differential fuzzing against `sha2`; review use at this security boundary before promotion. A checksum supplied beside a malicious download proves nothing about who published it. SHA-256 is not a signature and DX-SHA does not become a home-written signature implementation.

Recommend **Ed25519 detached signatures**, verified by a separately reviewed safe API in the distribution adapter, outside the strict core. Sign a fixed domain separator `dctx-plugin-manifest-v1` plus the exact manifest byte sequence. Preserve those bytes on disk; do not reserialize JSON before verification. Reject duplicate keys and invalid schema even after a valid signature. The manifest authenticates each target's digest, size, format, ID, version, capability set and protocol compatibility. Signing the checksum list alone would leave the manifest's capabilities unauthenticated. Signed auxiliary metadata either has its own domain or a digest in the manifest.

No exact verifier dependency is approved by this design. A prerequisite research packet must name its version, license, unsafe/native closure, target support and adversarial test vectors; first-party unsafe stays forbidden. If that exception is rejected, retain core functionality and verified bundled metadata, but do not ship an installer that treats unsigned remote code as verified. HTTPS similarly belongs to a reviewed distribution adapter: std alone does not supply TLS. Do not shell out to `curl`, `git clone`, package installers or an unpinned signature tool to claim zero dependencies.

The official distribution ships pinned official publisher/index public keys. A custom publisher's key must be approved from an independently verified fingerprint or an organization trust bundle; downloading a key beside its own signature is not trust establishment. Signing keys remain with the publisher, never in plugin artifacts. Host-managed trust metadata handles key rotation (old/new signed transition or explicit reapproval), revocation and a monotonic release sequence to reject unrequested downgrade/replay. Explicit rollback names an exact verified digest and reason; revoked artifacts cannot be re-enabled by ordinary rollback. Offline verification reports the last known revocation state and age, not “currently unrevoked”; an organization's freshness ceiling may refuse it.

### 5.3 Index and install transaction

Discovery is a **plain JSON file in a public repository**, with detached index signature and a versioned schema. Example record:

```json
{
  "schema": 1, "sequence": 12,
  "plugins": [{
    "id": "dctx/git", "repository": "owner/dctx-plugin-git",
    "summary": "Protect dctx exports from accidental Git addition",
    "publisher_key_fingerprint": "<approved fingerprint>",
    "releases": [{"version": "1.0.0", "manifest_url": "https://example.invalid/plugin.json",
                  "manifest_sha256": "<64 hex digits>"}]
  }]
}
```

These are illustrative locators, not existing published repositories. Index maintainers curate discovery, compatibility and provenance; inclusion is not permission to execute. Verify the publisher's signature independently of the index signature. Search text is untrusted display data, length-bounded and terminal-escaped. Cache and paginate the index (default maximum 4 MiB); do not fetch an executable merely to learn its description. GitHub public release metadata can locate the manifest without cloning a repository; API errors/rate limits are explicit and do not cause credential discovery. A direct URL/offline bundle bypasses discovery, never trust validation.

Install is `resolved -> downloaded -> verified -> staged -> installed-inactive -> activated`, with an operation lock and immutable version/digest directories. Default download cap is 256 MiB per artifact; organizations can lower or explicitly raise it. Installer network permission is limited to the selected public release origin and approved redirects, not a plugin runtime grant. Refuse URL userinfo, unsupported schemes and redirects to private/link-local destinations unless explicitly permitted by installation policy; never read ambient browser/Git credentials. Show requested origins before a download that adds one.

Stream the artifact into an owned staging directory, hash while reading, check exact size, signature, target and manifest/filename correspondence, then publish an immutable directory and atomically switch the host-owned activation record. The optional offline container is a bounded versioned table of named payloads, with lengths/digests and no compression in v1; allow regular entries only, never symlinks, device files, absolute/traversal paths or duplicate names. Partial downloads and malformed bundles never become executable. Installation does not run the plugin to “validate” it; negotiation occurs later under confinement.

Host revalidates the selected artifact before every launch and uses a platform launch primitive or protected install namespace that prevents a check/exec substitution. Where it cannot bind launch identity to verified bytes, it must refuse the claimed guarantee. Active workers lease their exact artifact version; update activates a new version for new sessions and defers removal of the old one until its leases end. Crash recovery keeps the last complete activation, classifies incomplete installs and reconciles pending intents. State format versions are explicit; keep versioned state or require a separately tested migration, never an executable install hook. Concurrent desktop and CLI management uses the same lock/registry rules.

### 5.4 Locations, offline operation and bundled updates

| OS | Immutable installed versions / private state | Registry, consent and trust |
|---|---|---|
| Linux | `${XDG_DATA_HOME:-~/.local/share}/dctx/plugins/<id>/<version>/<digest>/`; sibling `plugin-state/` | `${XDG_CONFIG_HOME:-~/.config}/dctx/plugins/` |
| macOS | `~/Library/Application Support/dctx/plugins/<id>/<version>/<digest>/`; sibling `plugin-state/` | `~/Library/Application Support/dctx/plugin-config/` |
| Windows | `%LOCALAPPDATA%\dctx\Plugins\<id>\<version>\<digest>\`; sibling `PluginState\` | `%APPDATA%\dctx\Plugins\` |

Encode IDs as validated path components, not unchecked string concatenation. Use private owner-only permissions/ACLs. Download staging lives under the same managed data root for atomic publication; download caches may use the architecture's per-OS cache root. Installed executables, consent and trust are not disposable map cache and are never evicted by `dctx cache gc`. A caller may supply an explicit portable plugin root; no project-local auto-discovery, global PATH scanning or system-wide privileged installation in v1. Existing configuration paths migrate by explicit recorded import, not by silently creating a second registry. Libraries receive locations explicitly and do not inherit the CLI's global registry.

The default CLI release contains `dctx/git`'s signed build manifest and worker implementation; `plugin list` shows `origin=bundled`, the containing binary digest and the plugin's independent version. The app does likewise, using its normal signed package/update system. The installed distribution's manifest authenticates the binary and embedded plugin inventory; no claim of a separately hashed embedded code slice is required. On first accepted installation, record the narrow consent profile and enabled default. If a plain copied binary has no such record, report needs-consent as in §1.

Bundled plugin updates arrive with normal signed dctx/app releases and are verified before activation. No startup downloads, timer, cron or silent independent replacement is introduced. `plugin update dctx/git` reports the containing-release update path for a bundled entry. A user may explicitly install an official standalone version as an override; it must carry the same canonical ID/signer, pass protocol/grant checks, and appear as `origin=override` with a clear revert-to-bundled action. Removal of an override returns to the bundled selection only if it was enabled before and consent still applies; an explicit disable/removal mask survives every update. App and CLI share user-installed selections only when they use the same explicit registry and compatible host API; each reports its own bundled version.

Default CLI and desktop releases must work offline after installation. Offline bundles contain the manifest, signature, exact platform artifact and relevant signed trust metadata; verification never requires a network callback. A strict distribution omits plugin host/worker/distribution adapters and all Git behavior, keeps pack/map functional, and reports plugin execution unavailable. This preserves the literal core promise while the default distribution discloses its reviewed host dependencies.

## 6. The first plugin, dctx-plugin-git

**`dctx-plugin-git` is explicitly a plugin.** It is shipped and enabled by default in the app and default CLI distribution, can be disabled or masked through `dctx plugin`, and is absent from `dctx-core` and the strict distribution. It owns local `.gitignore` protection and the request to run `git add .gitignore`. Dctx hashing, indexing, freshness, cache lifecycle and map commands do not depend on it or on Git. No `git_safety` compatibility function is reintroduced in core.

### 6.1 Historical behavior and deliberate changes

The historical `git_safety.rs` at `85c0f7a` checks for an existing `.gitignore`, scans for a trimmed exact `# [dctx v<CARGO_PKG_VERSION>]` line, and returns immediately on a match. Otherwise it collects trimmed nonempty noncomment lines, appends missing required patterns under `# [dctx] Automated Rules`, appends a version marker even if no pattern was missing, then runs `git add .gitignore` in the target directory. It discards the Git status/stdout/stderr. It does not create a missing `.gitignore`, remove old markers, test repository membership, detect negation conflicts or prove that rules remain present on its marker fast path. The old CLI calls it before writing local output when clipboard mode is off and the update flag is true.

The restored required pattern set and local marker syntax are:

```gitignore
# [dctx] Automated Rules
.dctx
.dctx/
*.llmcontext.txt
*.llmcontext-*-diff.txt
# [dctx vX.Y.Z]
```

`X.Y.Z` is the **host dctx release version**, preserving ADR-005's version authority; the independent plugin version and rule-set revision are recorded in receipts. Changing plugin versions cannot impersonate a different dctx release. An approved rule-set revision change invalidates the verification memo even if the host version is unchanged. There is no “0 ms” claim: reading and validating a bounded `.gitignore` and starting a worker have costs.

The following amendments to literal legacy behavior are explicit recommendations, not descriptions of old code:

| Legacy behavior | Proposed plugin behavior and reason |
|---|---|
| Marker alone assumes safety | Require current marker **and** complete rules with no known conflict. A marker is a compatibility stamp, not an authorization or integrity token. |
| Direct append may race and damage text boundaries | Preserve existing bytes/newline style, compute a bounded append, then use host compare-and-replace. A conflict returns `Conflict`; never overwrite a concurrent edit. |
| Writes even in a non-Git directory | Require a host-validated repository binding for automatic protection. No repository yields `skipped:not-a-repository`, with no mutation or Git spawn. |
| Every slow path stages, failures hidden | Stage once after a successful change if granted; report missing Git, denial and nonzero exit. Keep a receipt for a file edit even when staging fails. |
| Existing negations may defeat the patterns | Preserve user rules; report `rules-conflict` for known conflict/unsupported semantics instead of claiming safety or silently overriding it. |
| Installer edits global ignore/config | This plugin restores **local runtime** safety only. No writes to global Git ignore, user Git config or dctx config. ADR-005 installer/global merge behavior is outside this grant and needs a separate future capability/decision. |

### 6.2 Exact proposed operation

1. The facade emits `before-pack-output-written` only for a successful authorized plan that will create project-local text exports. Stdout/clipboard-only, cache-only pack, map, template and management commands do not trigger git safety. Absolute exports outside the project produce `skipped:outside-project`; the plugin receives no outside path. If an existing legacy update setting is false, migration records the plugin disabled. True maps to enabled preference, with consent still required; the old setting does not bypass the new policy. `create_missing=false` is fixed in v1.
2. Resolve the actual target project's `.gitignore` handle and repository administration binding without invoking Git. Never search parent projects to obtain a broader write scope. An ordinary repository at the target root is supported first. Linked worktrees/submodules, nested targets or administrative paths outside the granted root need an explicit additional binding and tests; otherwise return `unsupported-repository-layout`. No `.gitignore` means skip and no creation. A symlink, hardlink, reparse point, unreadable file, oversized file or excluded/omitted content returns a specific refusal without content access. Git executable availability is not required for editing: a validated repository may still receive local protection and a visible `git-missing` staging warning.
3. Through `file.read`, read at most 1 MiB of authorized UTF-8 `.gitignore` bytes and obtain a precondition token. Reject NUL/invalid encoding; preserve CRLF/LF and every existing byte. Recognize only complete known pattern lines and exact trimmed local markers; do not trim escaped whitespace in arbitrary user patterns. Check the four literal patterns and the planned output paths against supported ignore semantics, including later negations and nested rules already authorized by the host. If necessary rule files cannot be read under selection, return `coverage-unknown`; do not query Git or open excluded files to settle it. An ignored output directory protects its descendants; an unknown rule form cannot be assumed safe.
4. If all patterns and the current marker exist and planned exports have verified coverage, return `ok:already-protected` with **no write and no Git invocation**. Removing a required pattern while leaving the marker must invalidate this fast path. Verification may be memoized within one invocation using the unchanged file identity/precondition and rule-set revision, not indefinitely by marker alone.
5. Otherwise, append only missing literal patterns in the listed order, adding a newline separator if needed and `# [dctx] Automated Rules` only when adding rules. Append `# [dctx v<host_version>]` only if that exact marker is absent. Preserve old markers and all user lines. If only the marker is missing/outdated, append just the new marker. Re-evaluate the resulting planned coverage before committing. A later negation or custom output name outside the four patterns yields a warning/refusal, with no silent promise of protection. Additional custom ignore patterns are a future typed configuration extension, not guessed from arbitrary output filenames.
6. Upload the replacement through bounded `file.stage` chunks, then ask `file.replace` to commit that upload under the original precondition, with maximum growth 4 KiB and no create/delete. Repeated identical upload offsets are idempotent; conflicting chunks are refused. These actions and their results are logged. If concurrent content or policy changed, refuse and let a new invocation recompute; no loop that repeatedly overwrites a user's edits. After a successful change, the facade invalidates/reloads affected selection policy before pack content reads. If it cannot safely revalidate, it refuses the pack rather than using a stale selection.
7. If configured/granted, request `program.run` operation `git.add-ignore`. The fixed operand is `.gitignore`, equivalent to `git add -- .gitignore`, with cwd bound to that exact project root. The adapter may add only its reviewed isolation options, never plugin-supplied arbitrary Git flags. The host verifies that the file still has the just-written identity/content and preserves that binding through staging. A concurrent change yields `Conflict` or `staging-unknown`, not permission to stage the replacement. On success, record the exit/receipt and show `Git plugin: updated .gitignore for dctx vX.Y.Z; staged .gitignore`. No commit, push, status scan, checkout, reset, unstage or `git add .` is available.
8. On missing program, denied capability, Git failure or cancellation, report `warning:rules-written-not-staged` with the file receipt and safe reason. Do not undo the ignore edit or delete Git's lockfiles. A crash around staging is `EffectUnknown`, requiring reconciliation. The next automatic hook does not repeat staging merely because the previous one failed if rules/marker are now current. Explicit `dctx git protect --stage` can request a new, consented staging action for the verified current file; it is never an automatic replay of an uncertain action.

Staging applies to the **whole current `.gitignore`**, including pre-existing user changes, as with historical `git add .gitignore`. Consent must say that plainly. Do not claim that only dctx's appended lines are staged. Recommend default `stage=true` to honor decision 079, with `stage=false` available for users who prefer manual staging. Partial-index workflows receive the same warning and may choose edit-only. Unmerged `.gitignore` entries or an already locked index refuse staging. `.gitignore` does not untrack an already tracked export; the plugin reports this limitation without scanning or rewriting the index to remove files.

### 6.3 Git as a constrained external program

The capability is a named Git executable resolved once by the host from explicit configuration or an approved system search path, checked for identity/permissions and recorded in the grant. Never resolve a repository-local `git` or use a shell. Git is not downloaded with dctx and is not a runtime dependency of the strict core. The plugin worker itself does not spawn it; its request is fulfilled by the effect broker on the plugin's behalf.

`git-admin:stage-ignore` includes only the validated repository's administrative metadata needed for this operation, the index/lock transaction and object writes derived from the authorized `.gitignore`. It excludes broad worktree reads, arbitrary object reads, packed-history traversal, network, user credential/config homes, hooks, filters, helper programs and unapproved descendants. The program gets an allowlisted environment and immutable executable/runtime resources. Its output is bounded and classified by the host, not blindly copied to logs.

**Git configuration/attributes are an implementation gate.** Even `git add -- .gitignore` can consult attributes/configuration, follow config includes, apply clean filters and interact with repository extensions. A pathspec alone is insufficient. The platform research packet must pin supported Git versions and prove a recipe that disables external filters/hooks/helpers, prevents outside configuration reads, handles attributes without excluded content access, and confines repository writes. It must test malicious configs, attributes, shared object locations, linked worktrees and index extensions. Do not guess that one environment variable disables all of them. Where the safe recipe cannot support repository features, deny staging with an explicit reason and retain edit-only behavior. If an adapter cannot prove that Git staged the intended `.gitignore` under the selected transformation policy, it must not report `staged`.

The Git process's administrative access is visible as part of the consent record, not buried in an implicit `spawn` grant. No real repository test may use the developer's Git configuration or credentials. Git-mode tests run in private disposable repositories with fake user homes, explicit fixture config and confined processes; exit assertions inspect only the allowed resources. This proposal does not run Git effects or modify a repository.

### 6.4 Repository layout and tests

Recommend a first-party workspace crate, Apache-2.0, with independent plugin version/release metadata:

```text
plugins/dctx-plugin-git/
  Cargo.toml               # lib + optional standalone bin; internal protocol dependency
  plugin.json              # source manifest; target digests filled by release tooling
  src/lib.rs               # public plugin entry; forbids unsafe
  src/domain/              # pure marker/pattern plan and typed outcomes
  src/ports/               # broker client contract
  src/adapters/            # framed stdio client, no direct project IO or Git spawn
  src/main.rs              # standalone worker entry only
  tests/                   # conformance, historical parity and privacy fixtures
```

The default CLI/app composition layer links this crate behind `bundled-plugin-git`; the strict build excludes it. A workspace owner adds `plugins/*` membership; no plugin packet edits root Cargo files. A separate public GitHub repository may later host release artifacts/index entries, but source remains in the monorepo initially to make host/plugin protocol changes atomic and testable. No publication is authorized here.

Required tests cover:

- Historical parity: missing file stays absent; all four patterns appear; partial sets gain only missing entries; existing text survives byte-for-byte; old marker gains current host marker; current complete rules are idempotent with zero write/spawn; marker-only update stages once. A table records each deliberate amendment above, so parity does not freeze historical bugs.
- Correctness edges: removed rule with current marker, duplicate/old markers, no trailing newline, CRLF, UTF-8, invalid bytes, growth/size limits, negations/nested rules/custom output coverage, concurrent edits, failed replace and policy reload. Unknown coverage must not return protected.
- Privacy/effect refusal with positive controls: excluded/omitted `.gitignore` produces zero read/hash/write/spawn; no output bytes delivered; write-only does not grant read; wrong file, path traversal, symlink/hardlink/reparse swap, wrong root/operation/arguments, no program grant, network attempts and unbrokered process attempts are denied and logged.
- Lifecycle: disabled/default/needs-consent, non-Git and no-Git-installed cases, stdout/clipboard/map queries produce no hook effects, whole-file staging disclosure, nonzero Git exit, locked/unmerged index, partial/unknown result, repeated action IDs, cancellation and stalled descendants. No hidden retry of Git effects.
- Real Git fixtures per supported platform/version: only `.gitignore` is added to the index; no other source content is read; malicious filters/hooks/config includes do not execute; excluded attributes/objects cannot be read; nonstandard layouts refuse safely. Compare index before/after in fixture inspection, never through broad runtime Git status.
- Distribution boundary: same corpus through same-executable and standalone workers; official app/CLI list the plugin as bundled and enabled; disable persists after updates; strict dependency/source/spawn audits find no Git plugin or restored core Git behavior.

These are implementation exit tests, not tests claimed to have run for this design. Unit tests sit with the domain rules; integration/property tests and formal witnesses belong to the owning crates and §7 model packet.

## 7. Staged packets with own sets and exit tests

These are proposed packets for L1 to cut after the maintainer's decisions, not dispatched work or grants. They preserve DX-P1b, DX-SHA and the facade/DTS extraction boundaries in the architecture plan. No segment-store rewrite or new parser is a dependency of plugins. If the full facade extraction is not ready, first cut a narrowly owned facade seam; do not put the host back into `dctx-core` as a shortcut. Coordinate all reused ownership with architecture packets before dispatch.

Every implementation packet needs explicit edit/exec effects and its own build/temp/fixture directory, proposed as `.agent/validation/<packet-id>/` in addition to the literal source `own` set below. The directory is unique per packet; stdout/stderr evidence is captured by the kernel, not redirected to unowned paths. Leaf crates own their manifests; only PL-W owns root Cargo files. Component validation uses authorized isolated fixture workspaces until PL-W integrates the completed manifests. Never generate a lockfile or target tree in another packet's ownership. The shared suite is not repeatedly run by parallel workers: each runs its component gates, then PL-V runs the full product suite once at the end. A failed shared baseline stops feature promotion.

### 7.1 Contracts, feasibility and models first

| Packet | Literal source own set | Dependencies | Exit predicates / evidence |
|---|---|---|---|
| **PL-C — freeze v1 contract** | `docs/design/plugin-api/` | Eris §8 dispositions; DX-P1b target confirmed | Complete manifest/wire/DTS schema and valid/invalid fixture corpus; no placeholder digests in executable fixtures; requirement-to-predicate matrix covers §§1–6. No product behavior changes. |
| **PL-R — platform and trust research** | `docs/dossiers/dctx-plugin-platforms/`, `docs/dossiers/dctx-plugin-trust/`, `docs/dossiers/dctx-plugin-git/` | PL-C | Exact supported OS/Git/tool/library versions, licenses, unsafe/native closure, evidence for pre-exec confinement, no-network/descendants, race-safe file/launch binding, TLS and Ed25519 verification. Resolve Git config/attribute recipe and one-binary feasibility per target. Unsupported targets explicitly fail; no blanket “sandboxed” claim. |
| **PL-M — formal models** | `docs/formal/dctx-plugins/` | PL-C | Model checker passes PP1–PP10 below for lifecycle, grant revocation, action receipts and install activation; mutation configurations falsify each relevant safety property and liveness assumptions are stated. |
| **PL-P — protocol crate** | `crates/dctx-plugin-protocol/` | PL-C | std-only safe codec and types; `cargo test -p dctx-plugin-protocol`; clippy/format checks; frame fuzzing, split reads/writes, invalid JSON/length/depth/number tests, version negotiation goldens, replay/out-of-order/refusal positive controls; no IO in domain records. |

PL-R, PL-M and PL-P can run in parallel after PL-C. Before implementing a named external dependency/platform facility, read the resulting dossier and record the exact approved dependency closure. This fulfills know-before-use without pretending a design sketch is a researched API recipe. The critical-project requirement applies: formal checks are release gates, not optional documentation.

Numbered properties to model and give property tests:

1. **PP1 — authority only narrows.** Every accepted broker action belongs to the effective grant at admission and commit; configuration/update/revocation cannot widen it without a new consent/host decision.
2. **PP2 — no excluded source access.** An excluded/omitted/debug-only source has zero content reads, hashes or deliveries by host, worker or descendants; cached/derived outputs require current provenance authorization. Administrative exceptions remain enumerated and cannot deliver excluded source bytes.
3. **PP3 — effect audit.** Every committed external effect has a prior durable host intent; each completion or uncertain outcome links to it. No effect is admitted when audit capacity is unavailable.
4. **PP4 — protocol binding.** No effect occurs before confinement, verification, compatible negotiation and initialization; sessions/IDs/handles cannot cross artifact, project or policy-epoch boundaries.
5. **PP5 — no blind replay.** A repeated action key cannot execute twice within a reconciled ledger; unknown/partial outcomes never auto-retry. Conflicting arguments for one key are refused.
6. **PP6 — strict isolation.** Strict core/distribution closure contains no plugin worker or Git execution; plugin-free pack/map/library operations spawn no plugin or Git processes.
7. **PP7 — bounded resources.** Frame, queue, content, diagnostics, state and action use never exceed admitted quotas; over-limit input is rejected before unbounded allocation or an effect.
8. **PP8 — lifecycle progress.** Under finite admitted work, fair observations/broker execution, eventual dependency resolution and an OS that can reap, a worker completes, returns a typed refusal, or is cancelled/reaped. Slow advancing work is not killed merely for elapsed total time. A user halt revokes admission immediately.
9. **PP9 — install integrity.** Only a fully verified, compatible, trusted artifact can become active or execute; a crash/concurrent update yields the old or new complete activation, never partial bytes. Leased versions remain available until workers end.
10. **PP10 — hook order and result truth.** The effective plan gives an acyclic deterministic hook order; published engine outputs remain committed despite post-hook failure, and reported side effects agree with host receipts rather than plugin assertions.

### 7.2 Implement independent components

| Packet | Literal source own set | Dependencies | Exit predicates / evidence |
|---|---|---|---|
| **PL-H — host domain and brokers** | `crates/dctx-plugin-host/` | PL-P, PL-M | `cargo test -p dctx-plugin-host`; fake process/FS/audit tests for grant intersection, sequencing, policy revocation, replay/unknown receipts, backpressure, liveness feed and cancel/reap. Every deny has a positive control; all PP1–PP10 host transitions trace to model actions. |
| **PL-A — execution/confinement adapters** | `crates/dctx-plugin-platform/` | PL-P, PL-R and approved platform dependencies | Per-target subprocess adversary suite: direct source/home/network access, executable replacement, symlink races, unauthorized Git descendants and detached processes cannot escape. Broker path identity and no-secret environment tests. Failure reports unsupported, never unrestricted fallback. |
| **PL-D — DTS plugin plan** | `crates/dctx-template/` | PL-C, PL-P; architecture DX-B2 landed | V1 rendering parity; v2 plugin plan compile/load roundtrip; cycle/alias/unknown field/required-version denial; deterministic order; user disable and policy ceiling cannot be overridden; configuration-only plugin change does not invalidate map facts. Serializes after any other template owner. |
| **PL-G — git plugin domain/worker** | `plugins/dctx-plugin-git/` | PL-P, PL-R's approved Git contract | All §6 fixtures through a fake broker; standalone protocol conformance; marker/version/newline/negation/concurrency property tests; no direct fs/process access in worker. Historical differences have named expected outcomes. |
| **PL-I — installer/trust/discovery client** | `crates/dctx-plugin-distribution/` | PL-P, PL-R trust approval, DX-SHA | Fake network and signed fixture tests for SHA/signature/key/target failures, revoked/downgraded release, malformed offline bundle, traversal, download limits, interrupted/concurrent activation, persistent disable, offline freshness and capability-expansion consent. No code runs during install. |
| **PL-X — public index source/tooling** | `plugins/index/`, `tools/plugin-index/` | PL-C | Plain JSON index validates against schema; signed fixture verifies; duplicate IDs/alias claims, unsigned metadata, changed publisher and hostile display strings rejected. Produces reviewable local artifacts; **no repository publication**. |

PL-H, PL-A, PL-D, PL-G, PL-I and PL-X have disjoint source ownership and may run in parallel when their individual prerequisites are committed. PL-G does not need the real host to test its pure behavior. PL-H uses ports/fakes until PL-A exists. No leaf edits another component to make a test pass; a changed public contract returns to the sole contract owner and dependent work is recut. All first-party crates enforce `forbid(unsafe_code)` and public docs; approved adapter dependencies are separately inventoried.

### 7.3 Integrate and expose the product

| Packet | Literal source own set | Dependencies | Exit predicates / evidence |
|---|---|---|---|
| **PL-W — workspace integration** | `Cargo.toml`, `Cargo.lock` | PL-P/H/A/G/I committed manifests; existing root owner released | Authorized CI integrates plugin crates/`plugins/*`, resolves one consistent lock and checks default/strict feature closures; root membership/source closure is complete. Root-file ownership is explicitly granted to this packet, not inferred by leaf workers. |
| **PL-F — facade host integration** | `crates/dctx/` | PL-W, PL-H, PL-A, PL-D; architecture DX-D1 or explicit narrow facade-seam predecessor | Facade pack/index/cache hook corpus passes through real/fake ports; library isolation and read-only map calls show zero plugin/Git children; before/after commit errors truthful; policy revalidation after `.gitignore` change; no engine imports host internals. |
| **PL-CLI — plugin management and worker entry** | `crates/dctx-cli/` | PL-F, PL-I, PL-G | Every §5 command has help/JSON/success/refusal coverage; default bundled plugin listed; `dctx git`/canonical run routing works; `dctx .`/no-argument UI preserved; no effects from list/help; same-binary worker passes standalone conformance; strict build excludes git worker. |
| **PL-APP — desktop plugin surface/bundling** | `apps/dctx-desktop/` | PL-F, PL-I, PL-G; previous app owner released | UI shows bundled/default enabled state, capabilities, needs-consent and failures; enable/disable and updates call facade; worker mode before UI startup; signed packaging inventory; repository `e2e` skill evidence for touched UI, stored inside this own set. |
| **PL-REL — release tooling** | `tools/plugin-release/`, `docs/design/plugin-release/` | PL-W, PL-I, PL-X, PL-CLI, PL-APP | Local per-platform bundles/manifests/checksums/SBOM/provenance built reproducibly; test-key signature/offline install/update/rollback smoke tests; default one-executable CLI contains git plugin; unsupported platforms cannot be promoted. Real signing/publication requires a separate effect grant. |
| **PL-V — integration acceptance** | `tools/plugin-conformance/`, `docs/design/plugin-evidence/` | All above, DX-P1b, DX-SHA and green baseline | Run complete facade/CLI/app/default/strict conformance and PP1–PP10 mapping; real confined Git fixtures; dependency/native inventory; model checker with mutation witnesses; isolated benchmark report. At the end run **once** `cargo test --workspace --exclude dctx-desktop` in authorized VM CI, plus required lint/format gates and separately scoped desktop checks. No unchanged suite rerun. |

PL-CLI and PL-APP can run in parallel after PL-F and their other prerequisites. PL-REL waits for their final packaging interfaces. PL-V owns evidence/runners, not permission to fix other crates; failures produce narrowly owned corrective packets with changed inputs before a new validation invocation. Shared root files remain serialized with architecture/release work. Publish/index visibility, real release signing and app deployment are separate future effects; no cron or schedule is needed for any stage.

Promotion is incremental: protocol/model and feasibility first, then host with fakes, then explicit local plugin execution, then management/install, then the default git plugin in real CLI/app releases. External plugin execution and default Git effects do not ship on a platform until its privacy/confinement, trust and Git-recipe gates pass. If native confinement cannot satisfy the accepted requirements, return to §2's Wasm alternative with measurements; do not quietly weaken PP2 to keep a release date.

**Validation of DX-PLUGIN itself.** This packet is design-only with `effects=[read,edit]` and sole ownership of this Markdown file. Its full authorized acceptance set is the three supplied document predicates (nonempty file and sections 6/8), to be run together once after the final edit. A Cargo suite would execute code and write lock/target/temp artifacts outside that ownership, so it belongs to authorized kernel/VM validation, as in the architecture proposal. No green product suite, benchmark, sandbox implementation, signature verifier or visual test is claimed by this document. The future PL-V full-suite gate is distinct from the document check.

## 8. Decisions for the maintainer

There are **12 decisions**. decision 078's core Git removal and decision 079's default plugin requirement are settled; these choices specify how to fulfill them. Each recommendation is concrete enough to turn into PL-C's contract and implementation predicates.

1. **Execution and the one-binary promise.** Recommend the hybrid **out-of-process stdio mechanism**: bundled plugins are separate crates executed as workers of the same executable; independent plugins are managed external artifacts using the identical API. This preserves a single default CLI file while making external installation honest. Decline native dynamic libraries and defer Wasm unless confinement feasibility fails (§2).
2. **Native confinement as a release gate.** Recommend mandatory tested pre-exec confinement and brokered effects for every plugin, including the bundled one; no unrestricted trusted-publisher fallback. Approve the host-adapter dependency boundary in principle, with exact implementations reviewed in PL-R. If a target cannot meet it without first-party unsafe, that target's plugin release waits (§4.2).
3. **Wire contract and v1 scope.** Recommend bounded length-framed JSON, separately versioned negotiation, one invocation per worker, coarse lifecycle hooks and explicit plugin subcommands. Defer compact binary encoding, parser/store injection and concurrent hook effects until justified by evidence (§3).
4. **DTS and deterministic orchestration.** Recommend compiled DTS v2 plugin declarations with exact resolved artifacts, typed config, capability ceilings and acyclic ordering. Preserve v1 templates and library isolation. Required pre-hooks may stop an operation; default git hooks are advisory, and post-hooks cannot undo published output (§3.5).
5. **Administrative Git access under privacy.** Recommend a narrowly enumerated `git-admin:stage-ignore` grant in addition to `.gitignore` read/write and the named Git operation. It permits necessary index/control metadata and `.gitignore`-derived object writes, not excluded source/history reads. Organizations may deny it; unsupported repository/configuration features then yield edit-only or refusal. This is an explicit operational exception, never a blanket `.git` allowance (§4.3, §6.3).
6. **Default enablement and consent.** Recommend bundled-and-enabled by default with its exact grants disclosed/accepted in the normal app/CLI installation flow. A copied binary or migrated installation without consent reports `enabled / needs-consent`; a user disable persists through updates. Third-party install is inactive until enablement/consent. No template or unattended generic `--yes` can grant a new authority (§1, §5.1).
7. **Liveness and effects after failure.** Recommend progress/process/dependency evidence, explicit work budgets and cancellation, with wall-clock inactivity escalation only as the declared last resort. Require intent/result receipts, no blind retry after partial/unknown effects, and visible committed-output status on post-hook failure (§3.3–3.4).
8. **Release trust and distribution dependencies.** Recommend DX-SHA checksums plus Ed25519 signatures over exact manifest bytes, pinned publisher keys and explicit revocation/rollback policy. Approve researched HTTPS/signature adapters outside the strict core; do not implement new cryptography or equate a checksum with authenticity. An offline install must meet the same trust rules (§5.2).
9. **Discovery and updates.** Recommend a public plain-JSON index with detached signature, public GitHub/HTTPS release locators and a bounded offline bundle. Bundled plugin updates follow signed app/CLI releases; independent overrides are explicit. No startup fetch, auto-enable, cron or daemon. Platform install locations and immutable activation follow §5.3–5.4.
10. **ADR-005 restoration and safety amendments.** Recommend local-only behavior, the four exact patterns and host-version marker, existing-file-only writes, no core Git, and no global installer/config edits. Approve verifying rules despite a current marker, atomic preconditioned writes, no automatic edits in non-repositories, explicit coverage conflicts and reported Git failures instead of copying the historical gaps (§6.1–6.2).
11. **Default staging semantics.** Recommend `stage=true` for the official default, with explicit consent that `git add .gitignore` stages the whole file, including the user's existing changes. Allow `stage=false`; missing Git/denied staging leaves a successful ignore edit with a warning. Never commit, push, broadly stage, untrack exports or automatically repeat uncertain Git effects (§6.2).
12. **Ownership and promotion.** Recommend `plugins/dctx-plugin-git` in the Apache-2.0 monorepo, a pure protocol crate, host composed by the facade, and the staged disjoint ownership in §7. Require formal PP1–PP10 checks, real platform privacy fixtures, strict dependency audits and one final authorized full suite before default release. Publish/index/signing effects remain separate from implementation (§7).
