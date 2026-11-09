<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# DTS plugin declarations

Status: normative template source and compiled plugin form.
Extends DTS v2 and the existing ContextPlan; does not replace their grammar.
The template compiler resolves declarations against verified installed manifests/locks.

## Source grammar

Use the existing DTS v2 header/version mechanism. Within a v2 document, the
following sections are reserved. Values use JSON scalar/array syntax; comments
and whitespace follow DTS. A plugin section name is the literal canonical ID.
No duplicate section/property, unknown key, interpolation or executable value.

```ini
[PLUGINS:engineering]
enabled = ["dctx/git"]
order = ["dctx/git"]
network = "none"

[PLUGIN:dctx/git]
version = "1.3.0"
hooks = ["before-pack-output-written"]
required = false
allow = ["file.read:.gitignore", "file.write:.gitignore", "program:git.add-ignore", "resource:git-admin:stage-ignore"]
config.stage = true
config.create_missing = false
config.marker = "host-version"
before = []
after = []
```

One PLUGINS section per compiled organization plan. `enabled` and `order` are
unique canonical ID lists, at most 64. `network` is required and exactly `none`.
Each referenced available plugin has one PLUGIN section; each section has all
keys shown except `config.*`, whose omitted values use the manifest default.
`version` is exact semver, not a range, URL, branch or executable path. Resolve
install-time ranges before compiling. `hooks`, `before`, `after`, `allow` are
bounded unique lists; hooks are declared manifest names at payload version 1.
`required` is boolean. Manifest config fields are the only config.* keys.

`allow` is a ceiling selector over an already verified manifest, not a grant.
The compiler expands selectors to the manifest's typed capability records:

| Selector | Meaning |
|---|---|
| `file.read:<literal path>` | The named file read request |
| `file.write:<literal path>` | The named file write request, including its creation/size limits |
| `program:git.add-ignore` | The sole v1 program recipe, only with the next selector |
| `resource:git-admin:stage-ignore` | Explicit administrative ceiling for that recipe |
| `outputs.read:<role>:<operation>` | Narrow a declared output capability to that role and operation |
| `state:read`, `state:write`, `state:reclaim` | The respective operation of the declared state capability |

Unknown selectors or selectors not present in the manifest fail. The resource
selector alone grants no operation; program selection without the resource
fails. Empty allow means no broker capabilities. State source_content retains
the manifest request and must still pass provenance consent; no shorthand
permits automatic content persistence. Limits/liveness come from the caller
and compiled organization policy, not environment-expanded DTS strings.
Merge selectors of the same manifest request into one narrowed typed record
(for example state read+write booleans or selected output roles/operations).
Never duplicate a capability record to manufacture extra quota.

## Compilation and identity

`dts.schema.json` is the serialized **plugin extension** of ContextPlan, not
the complete template or organization language. Each entry carries an exact
available version/artifact/manifest digest/publisher/origin lock, or explicit
unavailable reason. All fields in the schema are required except those marked
optional by their schema. Hooks retain their explicit versions. Store the
plugin-plan digest alongside this extension; hash its canonical bytes as in
wire.md. Loading revalidates schema, locks and policy rather than trusting a
previously compiled byte blob. Never download, execute or resolve a moving
version during compile/load/run. Built-ins bind to the containing release digest.

Known missing optional plugins become unavailable entries. A missing required
plugin, unknown identity, incompatible required version/API, unknown field or
capability is InvalidConfig. Unavailable entries carry no guessed executable
identity, no effective hooks/allow and an empty config; required must be false.
Compilation does not enable the plugin or write consent. An available lock
must match the verified host registry; no project-supplied hash grants trust.

Config precedence is manifest defaults, explicit user settings, then compiled
project/organization values. Immutable deny/consent ceilings and a sticky user
disable always constrain the result; neither DTS `enabled=true` nor an update
can erase them. Missing optional capabilities degrade explicitly. One immutable
effective plan applies per invocation; policy changes cancel and re-admit it.

For each hook, expand order `[a,b,c]` into a→b and b→c on participating entries,
merge `before`/`after` edges, and topologically sort with canonical ID ASCII
order as tie-breaker. Reject self edges/cycles, duplicate IDs, contradictory
locks, unknown order entries and unknown required references. Known unavailable
optional predecessors are skipped and recorded unavailable; required missing
predecessors fail compilation. Project/plugin declarations cannot make an
unknown ID exist. Sequential execution only. Failed required predecessors skip
their transitive dependents; independent advisory failures do not. Preserve
earlier effect receipts if a later hook fails. Edges cannot alter selection.

Existing DTS v1 compiles/renders as before, with the default distribution's
plugin plan filtered by consent; embedding libraries default to **no host**.
The supplied `.dts` corpus contains section fragments for insertion under the
existing v2 container, not an invented outer version directive. Optional plugin
config changes only the plugin-plan digest, not source extraction/cache facts.
Strict/read-only operation ceilings still suppress plugin effects.

Command routing reserves plugin/pack/map/cache/template/help/version and all
internal switches. Refuse alias conflicts at enablement; never shadow built-ins.
`dctx plugin run <id> -- <args>` is canonical. `dctx git` is plugin dispatch,
not a Git command passthrough. Preserve `dctx .` and legacy path snapshots;
disambiguate an alias/directory collision with `./path` or `dctx pack <path>`.
Help reads verified manifests and starts no worker, Git process or download.
