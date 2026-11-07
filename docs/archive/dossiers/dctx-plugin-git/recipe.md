Historical design record; superseded by [the current documentation](../../../guides/PLUGINS.md).

<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Candidate recipe R1: raw-byte root .gitignore staging

PL-R · 2026-10-16 · PLUGINS §6.3 and PL-C git.add-ignore/admin contract.
Implementation specification for PL-G's broker recipe, subject to native proof.
Depends on platform/binding dossiers and the exact Git pins in README.md.

## Status and transaction

R1 deliberately supports only an ordinary root repository, SHA-1 format,
simple index, no attributes and controlled configuration. It stages the exact
authorized bytes without text conversion. Unsupported repository features
leave edit-only behavior; no attempt is made to bypass them by copying secrets
into an alternate repository. No runtime `git status`, `rev-parse`, config dump,
recursive attribute scan or history walk is part of admission.

This is a concrete candidate, **not a proven/approved executable recipe**.
PL-G/PL-A must qualify every fixed substep and make it one host-owned
`git.add-ignore` operation; none is a plugin-supplied command. R1 uses one
read-only `hash-object` substep to bind the intended object, followed by exactly
one `add` substep. This adds no wire method or arbitrary command authority.
If the owning implementation does not approve/support that internal substep,
refuse staging; do not replace it with a first-party SHA-1 implementation or
silently drop the intended-object check.

Hold an enforced resource/namespace transaction across admission, these two
substeps and result verification. It covers just-written `.gitignore` identity
and bytes, repository config/control files, attribute absence, index and object
destination. A dctx mutex or Git index.lock alone is insufficient against an
editor or external writer. [Bindings](../dctx-plugin-platforms/bindings.md)
explains the conditional-write gap. On ordinary mutable roots without a capable
provider or caller-protected namespace, R1 is **unsupported**.

## 1. Bind the repository without Git

The caller supplies the selected root's handle; don't search its parents.
Require `.git` to be a normal owned directory at that root and `.gitignore` an
authorized single-link regular file with the fresh post-edit precondition.
Reject symlinks, hardlinks, reparse/junctions, unexpected mount/volume crossings
and ambiguous case/Unicode aliases throughout the administration path.
On Windows, reject device/UNC surprises, alternate streams and reserved names.

Use metadata-only no-follow existence checks before content access. Refuse:

- `.git` indirection files, linked worktrees (`commondir`), submodules,
  external administrative/object directories and a repository rooted elsewhere;
- alternates/http-alternates, shallow/promisor/partial-clone markers or config,
  grafts/replace machinery requiring history, shared-object arrangements;
- an existing index.lock; never remove another client's lock or recover it;
- sparse/split indexes, unmerged entries, SHA-256 repository format, unknown
  repository extensions or unsupported index flags/extensions;
- root `.gitattributes`, `.git/info/attributes`, repository `.git/info/exclude`
  or any of their case aliases, regardless of whether their content looks benign.

The absence checks do not authorize reading excluded attributes. Even a benign
present attributes file is unsupported in R1. A target at the root has no
in-repository ancestor attributes to traverse; parent-project discovery is off.
The whole transaction prevents an attribute/control file appearing after check.

The index is narrowly permitted administrative metadata, internal to the host.
Bound its bytes/entry count before parsing (proposed R1 ceiling 64 MiB and
1,000,000 entries, lower organization limits win). Accept only valid index v2,
stage-0 regular entries, no extended/sparse/assume-unchanged/skip-worktree flags,
and **no index extensions** in the initial profile. Reject `.gitattributes`
entries as well: Git may fall back to indexed attributes if the working file
is absent. Conservatively reject such entries at any depth, including case
aliases. Do not decode their blobs to decide whether they are safe.

Absent index may be supported as an empty-index fixture only if the provider
has an absence-bound commit and administrative creation authority. Otherwise
require an existing index. Index checksum/format acceptance is also checked
by the qualified Git build; host structural parsing must not read beyond bounds.
Administrative paths/OIDs never become plugin-visible content or diagnostics.

## 2. Repository config: whitelist before Git parses it

There is no universal `GIT_CONFIG_*` environment switch that makes normal
`git add` ignore `.git/config`. `GIT_CONFIG_NOSYSTEM` suppresses system config,
not repository includes. `git -c` overrides values but does not prevent config
includes from being opened or unknown keys from affecting startup.

Read only bounded authorized `.git/config` through the host binding (proposed
R1 ceiling 64 KiB, 128 lines; HEAD ceiling 4096 bytes). Initial
R1 accepts a deliberately small grammar: ASCII section `[core]`, unique keys,
simple unquoted literal values, blank lines and bounded full-line comments.
Require `repositoryformatversion = 0`, `bare = false`; optional `filemode`,
`logallrefupdates`, `ignorecase`, `precomposeunicode`, `symlinks` are literal
booleans matching the qualified platform profile. Everything else refuses.
Pin this grammar's exact semantics against the Git fixtures; do not use regex
substring checks that miss quoted sections, escapes or continued values.

No subsection, include/includeIf, worktreeConfig, core.worktree, extensions,
filter.*, core.fsmonitor, credential/helper/ssh aliases, core.hooksPath,
core.attributesFile or alternative object configuration is accepted from the
repository. Repeated/case-aliased keys, quoting, continuation and malformed UTF-8
also refuse. A common repository with `[remote]` or `[branch]` settings is
therefore **edit-only in R1**, even if those settings would be harmless to add.
This intentionally small support set prevents a claim of universal Git support.
Do not rewrite the user's config to fit it or execute `git config` to preflight.

Read only the necessary bounded HEAD control bytes under the admin grant;
initially accept a standard local `ref: refs/heads/<validated-name>` form.
No automatic branch resolution/history access. An input requiring more control
reads needs a new qualified profile. Preserve existing repository files.

## 3. Build an empty environment, then add only required values

Values below are host-owned absolute protected resources, never worker strings:

```text
HOME=<empty private home>
XDG_CONFIG_HOME=<empty private config directory>
GIT_CONFIG_NOSYSTEM=1
GIT_CONFIG_SYSTEM=<owned empty regular file>
GIT_CONFIG_GLOBAL=<owned empty regular file>
GIT_CONFIG_COUNT=0
GIT_ATTR_NOSYSTEM=1
GIT_NO_REPLACE_OBJECTS=1
GIT_NO_LAZY_FETCH=1
GIT_TERMINAL_PROMPT=0
GIT_LITERAL_PATHSPECS=1
LC_ALL=C
```

Use explicit platform runtime necessities only (e.g. validated SystemRoot and
private TEMP/TMP on Windows). No ambient PATH search, proxies, SSH agent,
askpass, credential variables, GIT_CONFIG_PARAMETERS, GIT_INDEX_FILE,
GIT_OBJECT_DIRECTORY, GIT_ALTERNATE_OBJECT_DIRECTORIES, GIT_COMMON_DIR,
GIT_WORK_TREE, GIT_DIR, GIT_ATTR_SOURCE, GIT_TRACE*, GIT_EXEC_PATH, loader
injection variables or inherited file/socket/process handles. The add recipe
uses explicit `--git-dir`/`--work-tree` below instead of environment discovery.
Use argv APIs, no shell. A fake empty home is not permission to read the real one.

## 4. Bind expected object, then invoke add with fixed options

Run the exact pinned Git image under a **repository-free**, no-network,
no-descendant computation profile with private empty cwd and authorized bytes
as bounded stdin:

```text
<verified-git> hash-object --no-filters --stdin
```

No `-w`, path operand or environment repository override. Require success and
exactly one 40-character lowercase hexadecimal SHA-1 object ID plus newline.
This ID is a Git object identifier, not an authenticity check; DX-SHA SHA-256
still binds the artifact, source token and receipts. SHA-256 Git repositories
are excluded. No private source bytes other than the granted `.gitignore` enter
stdin. Pin the Git build's collision-detecting SHA-1 implementation as part of
its native inventory; refuse a build with an unqualified hash backend.
Cancellation/failure here cannot write repository objects or index.

Bind the expected loose-object path from that ID. Refuse existing expected
objects if satisfying the operation would require reading existing object
content; a matching name/hash does not authorize history reads. The provider
must constrain temporary/object writes to the authorized new blob transaction.
If a directory-wide rule would expose existing objects to writes, refuse that
repository or require an empty protected shard with exact commit validation.
Do not grant all of `.git/objects` merely because Git uses temporary names.

Then run, once, with cwd bound to the actual selected root:

```text
<verified-git> --no-optional-locks --literal-pathspecs
  --git-dir=<bound root>/.git --work-tree=<bound root>
  -c core.hooksPath=<owned empty directory>
  -c core.attributesFile=<owned empty regular file>
  -c core.excludesFile=<owned empty regular file>
  -c core.fsmonitor=false -c core.untrackedCache=false
  -c core.splitIndex=false -c core.sparseCheckout=false
  -c core.sparseCheckoutCone=false -c index.sparse=false
  -c core.autocrlf=false -c core.safecrlf=false
  -c core.fileMode=false -c core.ignoreStat=false
  -c core.checkStat=default -c index.version=2
  -c gc.auto=0 -c maintenance.auto=false -c submodule.recurse=false
  add -- .gitignore
```

This multiline block specifies an argv vector, not a shell script. Every path
comes from the broker. `--no-optional-locks` does **not** disable the mandatory
index transaction. core.fileMode=false prevents incidental executable-bit
changes to an existing regular `.gitignore`; a new one uses regular-file mode.
All options and their exact-version semantics require qualification, including
ones Git otherwise ignores on a particular operation.

No `--renormalize`, `--all`, `--force`, `--sparse`, arbitrary flags/pathspecs or
plugin executable paths. No filter name needs to be guessed/disabled: attributes
and indexed attribute fallback are absent, repository config has no filters,
and global/system configuration is isolated. `core.autocrlf=false` alone would
not defeat an `eol`, `text`, `working-tree-encoding`, `ident` or filter attribute.
There is no general environment variable that disables all attribute sources.

## 5. Program confinement and result binding

The Git profile differs from the worker's empty-filesystem profile. It permits
only immutable Git/runtime bytes, the exact authorized `.gitignore`, admitted
config/HEAD/index and metadata needed to address them, plus the bounded index
lock/replace and expected blob transaction. No excluded worktree content,
existing object/history/pack content, outside config/attributes, credential
homes, hooks, helpers, network or descendants. Do not expose packed histories
to compensate for an unexpected Git read. Fail the recipe instead.

Permissions to create/rename index.lock or a loose object require actual OS
rules and commit binding, not just plugin-level path strings. Directory-level
write grants must not permit mutation of other admin files. If the provider
cannot constrain Git's real operations that narrowly, staging is unsupported
on that target even if worker confinement itself passes.

Before reporting staged: confirm the `.gitignore` token/bytes/policy stayed
fixed; child exited successfully; index now has `.gitignore` at stage 0 with
the expected object ID and permitted mode; all other index entries' path,
stage, mode, OID and flags are unchanged; any stat-field changes and output
index extensions are only the exactly qualified ones. Validate the resulting
index structurally and keep the receipt tied to recipe/build/profile digests.
The successful Git fixture must additionally compare indexed blob bytes with
the authorized input through a separately authorized test oracle.

Do not read arbitrary repository objects at runtime to verify that claim.
The expected-ID substep, immutable input, no-transform policy, actual index
comparison and qualified Git implementation jointly establish it. If any part
is unproved, report `staging-unknown`/`EffectUnknown`, never staged. Unexpected
output extensions, nonzero exit after possible writes, interrupted audit or a
changed file require reconciliation, not automatic retry or an index rollback.
Bound/classify program output to PL-C's 65536-byte cap; never echo raw stderr
paths or administrative metadata to the worker/user.
