<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Bundled Git v1 profile

Status: normative bundled Git profile.
The Git plugin defines the algorithm; host adapters implement audited effects.
Core hashing, indexing, freshness, cache, pack and map never depend on Git.

The canonical plugin is `dctx/git`, implemented in `plugins/dctx-plugin-git`.
Its enabled default is advisory `before-pack-output-written`, triggered once
only by authorized project-local text exports. Clipboard/stdout/cache-only,
map/template/management commands do not trigger it. Outside-project exports
return skipped/outside-project without disclosing outside paths. Old update=false
migrates to disabled; update=true is a preference and never substitutes consent.

Required requests: `.gitignore` read <= 1048576 bytes; existing-file write with
growth <=4096 and replacement size <=1052672; create=false. Optional request:
program `git.add-ignore`, administrative resource `git-admin:stage-ignore`,
bounded output <=65536. Output content requests and network are empty. Config:
stage bool default true; create_missing enum [false]; marker enum [host-version].
Config cannot expand these fixed v1 values. Command verbs are status/protect:
status reports host/plugin-local protection state without Git status scanning;
protect may accept the sole option `--stage` for a new explicit staging request.
Unknown verbs/flags refuse; no Git passthrough. Whole-file staging must be
disclosed, including the user's pre-existing `.gitignore` edits.

Resolve an ordinary repository at the **target project root** without running
Git or searching parents for broader write authority. Other layouts need an
explicit reviewed binding or refuse unsupported-repository-layout. No repository
or no existing `.gitignore` => skipped, no creation/write/spawn. Symlink,
hardlink, reparse point, unreadable/oversized/invalid UTF-8/NUL or excluded/omitted
file => typed refusal before unauthorized content/hash. Read through broker,
obtain precondition and preserve all bytes, newline style and old markers.
Missing Git does not prevent an authorized local edit.

Required literals, in order, are `.dctx`, `.dctx/`, `*.llmcontext.txt`,
`*.llmcontext-*-diff.txt`. Header is `# [dctx] Automated Rules`. Current marker
is exactly `# [dctx v<host_version>]`; the host version is authoritative, never
the plugin version. Recognize complete known pattern lines and trimmed exact
markers; do not trim escaped whitespace in arbitrary user patterns. Verify
planned output coverage with supported ignore semantics, including negations
and authorized nested rules. Missing authorized evidence => coverage-unknown;
known negation/custom-name conflict => rules-conflict. Never read excluded rule
files or invoke Git to settle uncertainty. Already-tracked exports remain a
reported limitation; no untrack/index scan permission is implied.

If rules, current marker and coverage are complete, return ok/already-protected
with **zero writes or Git calls**. A current marker with a removed rule is not
valid. Otherwise append missing literals in order; add a newline separator
if needed and the header only when adding rules. Append the current marker
only when absent. Preserve old markers and all user bytes. Marker-only update
appends only that marker. Re-evaluate coverage before committing. No broad
rewrite, global ignore/config changes or silently invented custom patterns.

Stage bounded upload chunks and atomically replace under the original
precondition, size and growth limits. Policy/identity races => Conflict; do not
loop over the user's changes. After a successful edit, host reloads/revalidates
selection before any pack source read; inability to revalidate refuses pack.
Successful edit followed by staging failure keeps the edit and its receipt.

When stage=true and granted, request program.run with the just-written resource
and new precondition plus repository handle. The broker fixes the operand to
`.gitignore` (equivalent to `git add -- .gitignore`) and cwd to that exact root.
No plugin-supplied flags, shell or executable path. Check the just-written
identity/content through the staging action; changed file => Conflict or
staging-unknown. Resolve the Git executable once from explicit config/approved
system path, never repository-local PATH or a download. Scrub environment;
the recipe, not the plugin, owns any additional reviewed isolation options.

Administrative scope includes only necessary index/lock/control metadata and
object writes derived from the authorized `.gitignore`. It never permits
excluded source/history reads, broad worktree scans, config homes, credentials,
network, filters, hooks/helpers or unapproved descendants. Denial by organization
or unsupported repository features leaves edit-only behavior. Feasibility must pin
Git/OS versions and prove malicious config/include/attribute/filter, shared
object, worktree and index-extension handling before a real recipe is accepted.
The bundled confinement exception does not waive this program recipe gate.

| Outcome | Required visible truth |
|---|---|
| Successful edit and stage | File and program receipts; updated for host version, staged whole file |
| stage=false | File receipt only; no Git request |
| Missing Git / denied stage / nonzero exit | warning/rules-written-not-staged, file receipt and safe reason |
| Locked or unmerged index | Refused staging; no lockfile deletion or broad recovery |
| Crash/uncertain staging | EffectUnknown, no automatic retry or undo |
| Subsequent automatic invocation with current complete rules | already-protected, no retry of failed staging |
| Explicit protect --stage | New consented action for verified current file; reconcile earlier uncertainty first |

Tests use private disposable repositories and fake homes/config, never developer
credentials. Domain parity includes missing file, partial rules, removed rule
with marker, old/duplicate markers, no newline, CRLF, UTF-8, size/growth,
negation/nested/custom coverage, concurrent replace and policy reload. Real
platform tests prove only `.gitignore` is staged and forbidden reads/helpers do
not occur. Standalone and same-binary workers share the corpus and protocol;
there is no in-process Git shortcut. Strict dependency/source/spawn audits must
show no worker/Git in core or strict distribution.
