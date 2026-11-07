Historical design record; superseded by [the current documentation](../../../guides/PLUGINS.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Git qualification evidence plan

PL-R · 2026-10-16 · PLUGINS §6.4, PL-C PP2/PP4/PP9/PP10.
Acceptance cases for PL-G/PL-A/PL-V against candidate recipe R1.
All cases below are required future evidence; none was executed in PL-R.

## What must be recorded

Use disposable private repositories and fake homes, never the developer's Git
config, credential stores or live project. The owning validation packet must
authorize native processes, temp storage and its remote runner. Record exact
OS build/CPU/kernel/libc, Git executable digest/version/build flags, dynamic
runtime digests/licences, provider/profile/recipe revision, input file/index
digests and host-owned receipts. decision 080's bundled-worker exception does not turn
fake broker tests into evidence for the real constrained Git subprocess.

Each refusal needs a positive control on the same profile. Test outside-file
**opens/reads as well as output**: suppressing stderr after reading excluded
content is not privacy. A sentinel file alone can detect a write/exec side
effect but cannot prove there was no attempted or successful read. Use the
provider's authorized tracing/mediation or an equivalent test observation with
positive read controls. Do not silently grant privileged trace access at runtime.

## Real Git fixture matrix

| Case | Expected outcome / observation |
|---|---|
| Minimal admitted config, index v2, no attributes, unchanged resources | Exactly one fixed hash-object and one add; expected raw bytes staged, other index entries unchanged; complete receipts |
| LF, CRLF, no trailing newline, UTF-8, pre-existing user changes | Whole file staged byte-for-byte without CRLF/encoding/ident conversion; user changes disclosed in consent |
| Inherited HOME/XDG/system config with malicious include/filter | Real paths never opened; fake empty config positive control works |
| Local include/includeIf, quoting/continuation/case-obfuscated key, duplicate key | Preflight refusal before Git parses config; include destination unopened |
| Remote/branch sections in otherwise normal repository | Explicit unsupported R1 config; no quiet broadening of the whitelist |
| core.fsmonitor command/daemon, malicious hooksPath, filter process/clean | Preflight refusal; no helper or hook execution; test hooks even though add normally does not invoke most hook types |
| root .gitattributes or .git/info/attributes | Metadata-only refusal; content not opened, regardless of benign or malicious filter/text/encoding content |
| Missing worktree .gitattributes but staged .gitattributes in index | Refuse indexed fallback without reading its object; proves GIT_ATTR_NOSYSTEM is not treated as all-attributes-off |
| Global/system attributes and excludes | No outside opens; controlled empty files and absent repo info/exclude; mixed-case aliases tested on case-insensitive volumes |
| Attribute/config appears or changes after admission | Enforced transaction blocks change or provider refuses before mutation; never certify a post-hoc race as safe |
| Linked worktree, .git indirection, submodule, core.worktree, commondir | Explicit unsupported-repository-layout, no parent/outside admin reads |
| alternates/http-alternates, shared object dirs, promisor/partial clone | Refuse before existing object or network access; no lazy fetch/remote helper |
| SHA-256 format, sparse/split/index extensions, unmerged/skip-worktree/assume-unchanged | Explicit unsupported format/index state; no broad recovery |
| Missing index | Supported only with tested absence-bound admin creation; otherwise refuse, never silently create via a stale absence check |
| Existing expected object or populated shard needing broader writes | Refuse if narrow object transaction cannot be enforced; no history reads to verify matching OID |
| Existing index.lock | Refuse; retain lock bytes; no deletion/stale-lock recovery |
| Symlink/hardlink/reparse, parent rename, mount/volume switch | Refuse without following target or overwriting unrelated resource |
| Concurrent edit/remove/recreate of .gitignore/index/control files | Conflict or unsupported provider before effects; no silent clobber/recreation |
| Git path/DLL/loader replacement during hash, add or between them | Verified binding prevents substitution or refuses; both children use the same pinned image/runtime |
| Git/helper attempts network, shell, detached child, inherited-handle access | OS denial under the separately qualified Git profile; no child outlives invocation |
| add fails after writing blob or before index publication | Partial/unknown receipt preserved; file edit retained; no success claim/retry |
| Crash/cancel/audit failure around index commit | Truthful committed/unknown state and reaping evidence; never automatic rollback |
| Wrong target OID, changed unrelated index entry or unexpected output extension | Refuse staged claim; preserve effect evidence for reconciliation |

Fixture-only inspection may use explicitly authorized Git plumbing to read the
staged blob/index, but never copy that broader oracle permission into R1's
runtime grant. The test corpus's object contents are controlled public fixture
bytes. Seed an unrelated excluded path/object and prove it is never read by
the recipe. Measure both native architectures independently where applicable.

## Domain and host parity

Fake broker/property tests additionally cover no repository/missing file;
excluded/omitted content; invalid UTF-8/NUL/oversize; append growth limits;
current marker with removed rule; old/duplicate markers; marker-only update;
newlines; negation/custom-output/nested-rule uncertainty; already-protected
zero-effect path; stage=false; Git missing/denied; permission revocation;
selection reload after edit; forged/replayed receipts and cancelled upload.

Verify that automatic hooks do not retry a failed/unknown stage, and explicit
protect --stage reconciles uncertainty first. No management/map/clipboard/stdout
operation spawns the plugin or Git. Disabling the bundled plugin persists across
updates; external overrides must not acquire its pending-confinement exception.
Strict/core source and process inventories must contain no Git behavior.

## Reference locators and evidence limitations

Repository sources actually read: PLUGINS.md §6; plugin-api/{README,semantics,
git,distribution,properties}.md; ADR-005's historical behavior; Rust/code
standards; Cargo.lock for candidate Rust dependencies. PL-C supersedes the old
ADR's core implementation and marker-only fast path.

External reference locators, not fetched or claimed source-reviewed:
[Git 2.47.0 git](https://git-scm.com/docs/git/2.47.0),
[config](https://git-scm.com/docs/git-config/2.47.0),
[attributes](https://git-scm.com/docs/gitattributes/2.47.0),
[add](https://git-scm.com/docs/git-add/2.47.0),
[hash-object](https://git-scm.com/docs/git-hash-object/2.47.0),
[Git source v2.47.0](https://github.com/git/git/tree/v2.47.0),
[Git for Windows v2.47.0.windows.1](https://github.com/git-for-windows/git/tree/v2.47.0.windows.1).

The exact source audit must examine setup/config/includes, attr fallback,
conversion/filter handling, read-cache/index extensions, object writes and
child-process paths for each selected build. No repository-local Git source or
native trace was available in the named inputs. Therefore R1 cannot be promoted
on document checks alone, and no target/Git combination is currently claimed
qualified by these dossiers.
