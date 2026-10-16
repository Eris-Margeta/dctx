# Requirement-to-predicate matrix

PL-C · covers PLUGINS.md §§1–6 under decision 080/decision 081 precedence.
Every predicate below is a required future acceptance test, not a passed test
claim. Owners implement it in their own packet; PL-V aggregates the evidence.
Corpus IDs resolve through fixtures/catalog.json; Git goldens use git-cases.json.

Each predicate needs its accepted control and refusal/counterexample. `Platform`
means a real OS/Git fixture after PL-R's versioned research, not a mock assertion.
`Integration` means a future composed product test; the precise required
observation is given here so a later packet cannot replace it with a file check.
PP mappings and model mutation witnesses are in properties.md.

| Source / requirement | Predicate and required observation | Owner / supplied input |
|---|---|---|
| §1 one default executable | B-BINARY: bundled worker uses the verified containing executable, same protocol as standalone; inventory discloses any desktop helper | PL-CLI/PL-APP/PL-REL; Integration |
| §1 strict core, no unsafe | B-STRICT: default/strict dependency and source closure audit; strict omits host/worker/Git/distribution adapters; first-party forbid unsafe | PL-W/PL-V; trace-strict-isolation pair |
| §1 Git optional, decision 078 | B-NO-GIT: pack/map/hash/freshness/cache succeed with Git absent and with plugins disabled, zero Git spawns; no core git_safety shim | PL-F/PL-V; Integration |
| §1 one library contract/isolation | B-LIBRARY: CLI/app share facade; no explicit injected host yields zero discovery/config/program probes and successful pack/map | PL-F; trace-library-isolation-allow / without-host |
| §1 DTS organization control | D-CEILING: compilation cannot execute or consent; user disable and organization/caller ceilings dominate enablement | PL-D/PL-H; trace-grant-intersection pair |
| §1 privacy before content | S-PRIVACY: denied content has zero reads/hashes/deliveries, including cached and derived content | PL-H/PL-A; excluded/omitted/debug/cached-provenance trace pairs |
| §1 separate install/enable/consent | I-CONSENT: install inactive, copied bundled binary needs-consent, exact install consent can enable, sticky disable survives upgrade | PL-I/PL-CLI; Integration |
| §1 observable lifecycle | S-AUDIT / W-LIVENESS: host-owned grant/action/result evidence and advancing-work liveness | PL-H; durable-intent/progress-not-timer pairs |
| §1 discover/install publicly and offline | I-INTEGRITY / I-OFFLINE: HTTPS/GitHub locators and offline bundles meet same signed identity/trust gate; zero install-time plugin exec | PL-I; install-integrity pair; signed bundles by PL-I/PL-REL |
| §1 default Git visible/disableable | G-DEFAULT: bundled enabled preference listed; no consent means no effects; disable persists; strict reports unavailable | PL-CLI/PL-APP; Integration |
| §§1–2 excluded mechanisms | B-MECHANISM: no native dynamic loading, in-process shortcut, PATH script discovery, remote runtime, parser/store injection or arbitrary shell | PL-W/PL-V; closure and spawn audit |
| §2 coarse process lifecycle | B-WORKER: one worker per plugin/session, never per file, no daemon; same-binary entry before normal init and no project argv/cwd | PL-CLI/PL-A; Integration |
| §2 resource/performance evidence | B-PERFORMANCE: separate cold start/RSS/frame/Git-hook measurements by target and host/plugin cost; no speed threshold in normal tests | PL-V; isolated benchmark job |
| §3.1 dependency direction | B-BOUNDARY: protocol/domain std-only, host via facade ports, engines independent, no plugin type in core/map stable result | PL-W/PL-F; closure/source audit |
| §3.1 registry/lifecycle | W-STATE: resolve/verify/admit/negotiate/initialize/ready/invoke/shutdown/reap transitions only; broker before ready/second invoke refuses | PL-P/PL-H; initialize, wire-before-ready, wire-second-invoke |
| §3.1 identity/confinement/revocation | S-ADMISSION / W-BINDING: verified install identity is authoritative; pre-exec binding, revoke handles/admission immediately | PL-H/PL-A; third-party-confinement/bundled-exception/revoked-epoch pairs |
| §3.1 hook boundaries | W-HOOKS: all six invocation forms validate; once per successful engine boundary; no success hook on failed work or recursive write hook | PL-P/PL-F; invoke-* fixtures; Integration |
| §3.1 immutable output/input scope | S-PRIVACY: handles bind artifact/root/invoke/epoch/generation; no store paths; metadata pages filter authorization | PL-H; cached-provenance pair and output.list fixtures |
| §3.1 read-only map | B-MAP: lazy index inherits caller no-effect ceiling; map format/ranking/budgets/confidence unchanged | PL-F; trace-read-only-map pair |
| §3.2 frame length/stream handling | W-FRAME: bounded u32 LE, split prefix/body, zero/oversize/truncated/UTF-8 rejection before allocation | PL-P; frame-* including frame-hello |
| §3.2 JSON grammar/bounds | W-JSON / S-BOUNDS: duplicates, exponent/fraction/-0, >safe integers, nonobject, surrogate, trailing JSON, depth>32/members>4096 reject | PL-P; json-*, wire-too-large-integer, wire-negative-offset |
| §3.2 base64 and bytes | W-BYTES: canonical padding/pad bits, decoded size, offset/EOF arithmetic and encoded frame fit | PL-P; file/output/state result controls; wire-base64-*, wire-chunk-eof |
| §3.2 stdout/backpressure | W-LIMITS: stdout-only protocol, bounded independent stderr drain/queues/outstanding work; output flood cancels without unbounded memory | PL-P/PL-H/PL-A; byte-budget pair; Integration flood test |
| §3.2 negotiation | W-NEGOTIATE: highest common version, matching identity, required API subset, no unnegotiated extension; incompatible offer fails before access | PL-P; hello, hello-selected, hello-error, wire-unnegotiated-extension |
| §3.2 envelope/directions | W-BINDING: h:/p: uniqueness, exact pending response ID/session/version, event sequence, closed fields and exactly one result/error | PL-P; wire-wrong-*, wire-response-*, wire-unknown-*, wire-duplicate-response |
| §3.2 method/result discrimination | W-METHODS / W-RESULT: each request maps to its exact result schema; generic response union is insufficient | PL-P; all request-*/result-*, wire-response-wrong-method |
| §3.2 declared result/output schemas | S-CONFIG / S-RESULT: declared reason/diagnostic codes, bounded typed command records, no command output from hooks, known owned receipts | PL-P/PL-H; result-invoke, forged-receipt pair; Integration output validation |
| §3.2 typed failures | W-ERROR: all 18 codes round-trip, closed phases/effect state, no raw stack/path; zero-exit without terminal response fails | PL-P/PL-H; error-denied, hello-error; Integration error enumeration |
| §§3.2–3.3 committed truth/retry | S-RESULT / S-REPLAY: required pre-hook can veto; advisory warns; post-commit preserves output; partial/unknown never auto-retries | PL-H/PL-F; output-commit/unknown-replay pairs, wire-error-unsafe-retry |
| §3.3 output.read/list | S-PRIVACY: pin/check provenance and current generation per chunk/page; cursor cannot cross invocation/list type or expose outside paths | PL-H; output.read/list fixtures; Integration stale/revoked cursor tests |
| §3.3 file.read | S-FILE: named independent read/Include permission; continuation binds first token; change refuses without new content | PL-H; file.read fixtures, file-race pair; Integration continuation |
| §3.3 stage/replace | S-FILE: contiguous bounded sealed upload, identical replay idempotent/conflict refused, atomic compare-and-replace, no ungranted creation, cleanup | PL-H/PL-A; file.stage/replace fixtures, file-race pair; Platform race tests |
| §3.3 program.run | G-RECIPE: fixed identity/args/root/admin/env/output; no shell/network/unapproved descendants; checked intended staged file | PL-A/PL-G; program.run, wire-git-arbitrary-args; Platform |
| §3.3 plugin state | S-STATE: namespace/ownership/quota/lease/provenance version checks; own unpinned reclaim only; no automatic source persistence | PL-H; state.* fixtures, state-namespace pair |
| §3.3 replay/recovery | S-REPLAY: same key+args returns receipt, changed args refuses; pending intent reconciles or stays unknown; no exactly-once crash claim | PL-H/PL-M; replay-key/unknown-replay pairs |
| §§3.3–3.4 progress/cancel/probe | W-PROGRESS / W-CANCEL: monotone counters/sequences, known waits, challenge echo, revoked admission, bounded cleanup and visible reap_pending | PL-H; events/probe/cancel, progress negatives, cancel-revocation pair |
| §3.4 liveness | W-LIVENESS: fake advancing slow work survives; heartbeat/phase cycles/flood/dead wait/exit/refusing descendant cannot fake health; no default runtime kill | PL-H/PL-A/PL-M; progress-not-timer pair; fake process traces |
| §3.4 unattended policy | W-LIVENESS: explicit fail-closed or bounded last-resort observation plus reap grace; unknown activity not zero; force only after evidence | PL-D/PL-H; dts-inactivity-policy, dts-interactive-policy, dts-unbounded-inactivity |
| §3.5 source syntax/v1 parity | D-SOURCE: compile/load source fragments, reject duplicates/unknown selectors/interpolation; v1 rendering unchanged and libraries omit host | PL-D; dts-source*; existing v1 golden suite |
| §3.5 config/defaults/locks | D-COMPILE / S-CONFIG: exact verified version/digests, typed bounded defaults, optional unavailable explicit, required missing refused, no runtime fetch | PL-D; dts-plan, dts-optional-unavailable, dts-required-unavailable, manifest-config-types |
| §3.5 order/precedence | D-ORDER / D-CEILING: merge edges, reject cycles/unknown required refs, ASCII ID tie-break, sequential hooks/dependent skips, sticky disable | PL-D/PL-H; dts-*cycle*, dts-unknown-order, dts-duplicate-plugin; Integration |
| §3.5 plugin-plan digest | D-DIGEST: canonical plugin-plan hash separate from extraction/selection; plugin-only config changes do not reparse map | PL-D/PL-F; digests.json, dts-plan; Integration cache spy |
| §3.5 routing | D-ROUTING: reserved/conflicting aliases refuse, canonical run works, paths disambiguate, help starts no workers/search/download | PL-CLI/PL-D; manifest-reserved-alias / duplicate-alias; Integration |
| §4.1 manifest completeness | S-MANIFEST: closed bounded schema, semver/range/ID uniqueness, platform/runtime correspondence, all config/capability/limit/output declarations | PL-P/PL-I; manifest-git and manifest-* negatives |
| §4.1 semantic grant intersection | S-GRANT / S-CAPABILITY: M∩U∩O∩H∩S∩A at admission/commit, deny wins, min quotas, no implication from write/program to read/create/network | PL-H/PL-M; grant-intersection/revoked-epoch pairs; manifest cap negatives |
| §4.1 consent/update expansion | I-CONSENT: concrete digest-bound records; equal/narrower derived updates only under prior policy; signer/scope/capability expansion suspends | PL-I/PL-H; Integration consent/update spies |
| §4.1 path identity | S-FILE: no traversal, absolute/prefix/case/reparse/ADS/hardlink/symlink escape; current checked identity at commit | PL-A; manifest path negatives; Platform race fixtures |
| §4.2 native confinement | S-ADMISSION: third-party enable/spawn unavailable without tested pre-exec denial of FS/home/cache/network/descendants/secrets; no trusted-signer fallback | PL-A/PL-M; third-party-confinement pair; Platform adversary |
| §4.2 decision 080 exception | S-ADMISSION: only official verified bundled Git may report pending confinement with broker/audit; installed/override/spoofed ID cannot inherit it | PL-H/PL-A; bundled-exception pair; Integration provenance spoof |
| §4.2 safe adapters | B-ADAPTERS: exact approved dependencies/native inventory, no first-party unsafe, safe race/launch binding; unsupported targets explicit | PL-R/PL-A/PL-W; research inventories and Platform |
| §4.3 selection/cache/state | S-PRIVACY: current selection before read/hash/delivery, whole derived output denied or safely projected, cross-root state isolation | PL-H/PL-A; privacy/provenance/state trace pairs |
| §4.3 Git administration | G-ADMIN: separate consent to enumerated metadata/object-write recipe; no .git contents/paths/history/excluded source delivered | PL-R/PL-A/PL-G; git-staging-grant pair; Platform |
| §4.4 audit content/durability | S-AUDIT: host seq/time/IDs/digests/grants/bytes/receipt links, durable intent before delivery/effect, no secrets/source/raw output in persistence, unresolved segments pinned | PL-H/PL-I; receipt, durable-intent pair; failure injection and log-content spies |
| §5.1 search/list/info/help | I-MANAGEMENT: typed bounded JSON and help, verified metadata/cache age/grants/locks/receipts visible; no project writes/plugin/Git launch | PL-I/PL-CLI/PL-APP; Integration per-command tests |
| §5.1 install/enable/disable/remove/run | I-MANAGEMENT / I-CONSENT: each success/refusal, install inactive, scoped consent, disable cancels, remove waits/masks, run uses ordinary admission | PL-I/PL-CLI/PL-APP; Integration per-command tests |
| §5.1 update/verify | I-MANAGEMENT / I-INTEGRITY: explicit refresh/check, quarantine invalid, no code during verify, preserve old activation, persistent disable | PL-I/PL-CLI; install-integrity/activation-crash pairs |
| §5.1 repository != identity | I-IDENTITY: transferred locator cannot replace pinned publisher/plugin; mismatch refused and unknown publisher requires independent key approval | PL-I; signed negative bundles in owning packet |
| §5.2 SHA identity | S-ARTIFACT: dctx-hash exact bytes/length, one-shot/incremental; different payload or size refuses, no placeholders | PL-P/PL-I; bytes/nist.json, digests.json, git-cases.json, artifact negatives |
| §5.2 decision 081 signatures | I-SIGNATURE: first-party safe-Rust Ed25519 strict vectors, domain/exact-byte verification, dev-only oracle; signing only audited release tooling/key manager | PL-I/PL-REL; verifier vectors and signed bundles in owning packets |
| §5.2 trust/revocation/rollback | I-TRUST: independent pinned trust, authenticated rotation, monotonic sequence, no revoked rollback, offline age/freshness truthful | PL-I; trusted/untrusted/revoked/rotated/rollback fixture bundles |
| §5.3 index | I-INDEX: schema/size/signature, unique identity/versions, hostile text escaped, publisher verify independent, no discovery execution | PL-X/PL-I; index, index-duplicate-id; signed index fixtures in PL-X |
| §5.3 origin/download policy | I-ORIGIN: selected HTTPS/public redirect origin, no userinfo/ambient credentials/private destination, streaming caps and exact payload correspondence | PL-I; index-userinfo-url/http-url; fake HTTPS/DNS redirects |
| §5.3 offline container | I-BUNDLE: DCTXPLG1 table/length/hash bounds, exact payload totals, no traversal/duplicates/compression/special entries/install script | PL-I; bundle.schema.json plus hostile signed containers in owning packet |
| §5.3 immutable activation | I-ATOMIC: operation lock, old-or-new complete activation across interruption/concurrency, no install exec, lease old versions until reap | PL-I/PL-M; activation-crash pair; fault injection at every transition |
| §5.3 launch/state migration | I-LAUNCH: revalidate and bind checked bytes to launch; no substitute/mutable code; explicit versioned state, no install/migration scripts | PL-A/PL-I; Platform executable-race fixture |
| §5.4 locations/isolation | I-LOCATIONS: per-OS roots/private ACLs, validated components, same-root staging, explicit portable/library roots, recorded import; no auto-discovery/cache eviction | PL-I; platform fixtures and registry/cache spies |
| §5.4 bundled/override/offline | I-BUNDLED / I-OFFLINE: containing release authenticates inventory/digest, explicit override, sticky mask on revert, no startup fetch/schedule, installed offline works | PL-I/PL-REL/PL-CLI/PL-APP; Integration |
| §§6–6.1 ownership/parity | G-PARITY: plugin-only local behavior, exact four literals/header/host marker, preserve bytes; deliberate amendments tested separately | PL-G; git-cases.json |
| §6.2 trigger/scope/repo | G-TRIGGER: only project-local text exports, no stdout/clipboard/map hooks; missing/nonrepo no effects, unsupported layouts refuse, no parent widening | PL-F/PL-G; missing-ignore/no-repository cases; Integration |
| §6.2 read/encoding/coverage | G-COVERAGE: bounded UTF-8/no NUL, selected rule data only, negation/nested/custom coverage unknown/conflict cannot claim protected | PL-G; negation-conflict/coverage-unknown/excluded-ignore; further encoding/boundary fixtures |
| §6.2 idempotency/version | G-IDEMPOTENT: complete current marker/rules/coverage => zero write/spawn; removed rule invalidates; old/duplicate markers preserved | PL-G; marker-idempotency trace pair; current-complete/old-marker/partial-set |
| §6.2 append/commit/reload | G-REPLACE: missing literals only, LF/CRLF/no trailing newline, growth cap, upload CAS, no race overwrite; selection revalidation before pack reads | PL-G/PL-H/PL-F; text goldens, file-race pair; Integration reload failure |
| §6.2 staging/truth | G-STAGE: whole-file consent, stage=false/missing/denied/nonzero warnings keep file receipt; current rules do not silently retry uncertain stage | PL-G/PL-H; stage-disabled/git-missing, staging-grant/unknown-replay pairs |
| §6.2 fixed Git profile | G-PROFILE: fixed config/create=false, no output contents/network; only declared status/protect flags, no commit/push/broad stage/untrack/global config | PL-G/PL-H; manifest-git/widen-git-create, wire-git-arbitrary-args |
| §6.3 program confinement | G-RECIPE / G-ADMIN: researched exact Git identity/version/args/cwd/env/config/attributes/index resources; malicious hooks/filters/includes/shared objects deny | PL-R/PL-A/PL-G; real Platform fixtures |
| §6.4 packaging/test parity | G-DISTRIBUTION: same corpus through standalone/same-binary workers, visible default/disable, strict source/dependency/spawn audits; disposable test homes only | PL-G/PL-CLI/PL-APP/PL-V; Integration and Platform |

PL-C's actual kernel predicates are only directory existence and nonemptiness.
The directory/schema/fixture artifacts make the contract reviewable; they do
not satisfy any platform, formal, Rust, cryptographic or product predicate above.
PL-V runs the complete authorized remote workspace suite once after all owners'
component checks and formal/platform evidence are ready. No unchanged rerun.
