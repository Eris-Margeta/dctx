Historical design record; superseded by [the current documentation](../../../guides/PLUGINS.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# dctx-plugin-git dossier

PL-R · researched 2026-10-16 · repository and existing knowledge; no network.
DX-PLUGIN §6/§7.1 and PL-C Git contract; consumed by PL-G/PL-A/PL-V.
This records a narrow candidate Git recipe and release gates, not passing tests.

## Disposition and boundaries

**Real Git staging is not qualified on any platform by this packet.**
Use the candidate [recipe](recipe.md) only after its exact Git build, constrained
program provider, file transaction and [fixtures](evidence.md) are accepted.
Until then return an explicit staging refusal and preserve any successfully
authorized ignore-file edit. Do not label an ordinary `git add` invocation
confined merely because its sole operand is `.gitignore`.

decision 078: domain behavior lives in `plugins/dctx-plugin-git`; the host composes
explicit named-operation adapters. No Git in dctx-core, map, hashes, freshness
or cache; no Git fallback if the plugin is disabled or absent. The worker asks
the broker to act; it never gets direct filesystem or spawn authority.

decision 080's official bundled worker may report pending confinement, but this does
not waive Git's separate constrained-program recipe, privacy or safe file
binding. An installed override never gets the bundled exception. Missing or
unqualified Git does not prevent core operations or an independently safe edit.
If file binding itself is unsupported, refuse the edit as well; “edit-only” is
not permission to use an unsafe pathname transaction.

## Version and native dependency policy

| Platform | Exact initial qualification pin | Support status / runtime inventory |
|---|---|---|
| macOS arm64 and x86_64 | Upstream Git 2.47.0, separate native builds | Unqualified; record signed/approved executable digest, build flags, libSystem, zlib/iconv and any other linked library. Apple Git version suffixes are separate builds, not equivalent pins. |
| Linux x86_64 and aarch64 GNU | Upstream Git 2.47.0, glibc 2.39 qualification image | Unqualified; native ELF/loader and linked zlib/libc/other features require a complete digest/version inventory. No shell/perl/python/helper runtime permission implied. |
| Windows x64 | Git for Windows 2.47.0.windows.1 | Unqualified; inventory exact Git.exe/runtime DLLs and packaging. A Git for Windows installation is more than Git.exe; no inherited MSYS shell/helper authority. |

These historical pins make reproduction specific; they are **not** current
security recommendations or a `>=2.47` support promise. No exact build digest
or current advisory evidence is present in this offline research. Promotion
requires maintained patched builds and new qualification on changed inputs.
All other versions, vendor patches and target variants refuse staging until
separately qualified. A version string alone is not executable identity; never
run an unverified program's `--version` to decide whether it is trustworthy.

Git upstream's primary licence is GPL-2.0 (COPYING includes its linking note
for OpenSSL); bundled components and Git for Windows packages carry additional
licences. It is native C code, not covered by Rust forbid-unsafe. No `git2`,
libgit2 or gix dependency is selected. Exact dependency/native licences depend
on the chosen Git build and are unresolved here. Git stays an independently
installed optional system program, not linked or bundled into Apache-2.0 dctx.
If a later release redistributes Git, source/notice obligations need an explicit
release inventory and decision; this packet grants no redistribution.

## Frozen plugin behavior

[PL-C git.md](../../../design/plugin-api/git.md) remains authoritative. The default
hook is advisory before authorized project-local text exports. No invocation
for map/cache/template/management, stdout or clipboard output. No parent search
to broaden the selected project; no repository or no existing `.gitignore`
means skip without creation. A copied binary still needs recorded consent.

Read authorized UTF-8 `.gitignore` up to 1,048,576 bytes, reject NUL, links,
reparse points, omission/exclusion or unproved coverage; preserve all user bytes,
LF/CRLF and old markers. Append only missing literals in this order:

```gitignore
# [dctx] Automated Rules
.dctx
.dctx/
*.llmcontext.txt
*.llmcontext-*-diff.txt
# [dctx vX.Y.Z]
```

The marker uses the **host** version, not the plugin version. Verify actual
rules/negations/output coverage despite a current marker. Unknown or inaccessible
rule evidence yields coverage-unknown; known conflicts yield rules-conflict.
Complete current protection performs no write and no Git spawn. Bounded append
has growth <=4096 and replacement <=1,052,672 bytes, existing-file-only under
the original precondition. Revalidate selection after an ignore edit before
any pack content read. Failed revalidation blocks pack.

`stage=true` stages the whole `.gitignore`, including the user's pre-existing
changes, only with explicit file/program/admin consent. `stage=false` has no
program request. Never commit, push, broadly stage, untrack exports, edit global
ignore/config, run hooks or automatically repeat an uncertain Git effect.
Already-tracked exports remain a limitation, not authorization for index scans
outside the narrow internal recipe.

## Expected result handling

Keep file and program intent/result receipts separate. Missing Git, denied or
unsupported staging and a classified pre-effect failure produce
`warning/rules-written-not-staged` when the edit succeeded. Failure after possible
object/index writes retains partial/unknown effect evidence; exit nonzero does
not prove nothing happened. Crash/uncertain stage is `EffectUnknown`, no retry
or blind rollback. A later automatic current-marker run does not retry staging.
Explicit protect --stage is a new action after prior uncertainty is reconciled.

One-binary feasibility is the **bundled Rust worker** in dctx's executable,
not bundling Git. The host never downloads Git or installs an interpreter.
Unsupported OS/Git/runtime/profile states are visible; the strict distribution
omits Git behavior and still provides pack/map.
