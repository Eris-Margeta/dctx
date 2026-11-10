<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# What dctx is

dctx turns any folder into the smallest correct context for a task, offline, with byte-stable output that prompt caches can reuse.

“Smallest correct context” describes the goal, not a proof that task selection
is complete or minimal. The shipped engine selects lexical symbols and document
sections, verifies source reads and bounds answers. A 200 GB folder needs a
relevant sub-root and ignore rules; output budgets do not bound scanning.
Byte stability enables reuse, while provider cache rules determine actual hits.
dctx has no embeddings, network, telemetry or agent loop in its pack/map/MCP
workflows. It does not call a model or execute the task for you.

> dctx is an advanced tool with several ways to use it. Used well, it cuts the context an agent needs to a fraction. Used without a plan, it can add calls and tokens instead of removing them. If you are new, follow the safe path: one project, the system binary, the lean MCP profile. Measure before you change anything.

## Choose your path

Choose one of these four paths. Each starts with a shipped workflow; the planned
scope and distribution features are labeled explicitly.

1. [One giant folder](#one-giant-folder)
2. [Many small projects](#many-small-projects)
3. [Agent builder embedding a library](#agent-builder-embedding-a-library)
4. [Team / enterprise admin](#team--enterprise-admin)

### One giant folder

One giant folder (e.g. 200 GB): system binary; per-root ignore policy; name a sub-root/--place in tasks. Pitfall: pointing agents at the whole root.

Install the [system binary](#safe-path-in-ten-minutes), choose a relevant package,
and put exclusions in that root's `.dctxignore` before the first query:

```sh
dctx map find Config --place packages/service-a --budget 500 --limit 5
```

Expect bounded symbol IDs and line ranges, not a complete answer about the whole
parent tree. Use an ID with `map read`. There is no general receipt yet;
`--json` exposes freshness, precision and pagination. Each invocation checks
source stamps; no daemon or file watcher is used. Catalog-first sharding and a
unified policy file are planned. Read the
[huge-tree limits and ignore example](https://www.dctx.internet.technology/docs/modalities/#huge-trees-scope-before-indexing)
before warming an index; `--in` only filters after indexing.

### Many small projects

Many small projects: system binary; lean MCP once in the agent config; nothing per project. Pitfall: stale caches of deleted projects -> cache gc/purge.

Install one binary and configure the [lean MCP profile](https://github.com/Eris-Margeta/dctx/blob/main/docs/guides/MCP.md) in your
agent. The config can live at user scope; launch the server with the intended
project root for each session. No project dependency is required. Start with a
`find` call followed by `read` using its exact ID. Expect source plus freshness
and precision in JSON, not a general receipt or session delta.

```sh
dctx cache status --json
dctx cache gc --dry-run
dctx cache purge --root /absolute/deleted-project --dry-run
```

Inspect the dry run before applying cleanup; `purge` requires `--yes`.
See [cache controls](https://www.dctx.internet.technology/docs/map-mode/#index-cache-and-privacy). Indexes live in an
engine `major.minor` subdirectory (for example `1.3`) of the cache parent, which is
`DCTX_CACHE_DIR` or the platform default `v2` cache root; patch releases share it, and
data from other engine versions is ignored.

### Agent builder embedding a library

Agent builder embedding a library: project library with its own pinned binary (Python, Node or Go wrapper over `dctx rpc`); never parse or slice pack text. Pitfall: re-implementing engine logic.

The [Python](https://github.com/Eris-Margeta/dctx/blob/main/packages/python/README.md), [Node](https://github.com/Eris-Margeta/dctx/blob/main/packaging/npm/dctx/README.md)
and [Go](https://github.com/Eris-Margeta/dctx/blob/main/packages/go/README.md) wrappers over `dctx rpc` are built and tested in
CI. Install Node with `npm install @tejl/dctx`, which includes a pinned
`@tejl/dctx-<platform>` optional dependency; use CI artifacts or a checkout for
Python and Go as their READMEs describe. Alternatively, pin the
[Rust facade](https://www.dctx.internet.technology/docs/modalities/#rust-library-embed-the-facade) from tagged Git,
or invoke the installed CLI as a subprocess. The first runnable Rust example is
in the [library doctest](https://github.com/Eris-Margeta/dctx/blob/main/examples/library/README.md). For subprocess use:

```sh
dctx map find Config --json --budget 500 --limit 5
```

Expect JSON records with IDs and precision/freshness metadata; pass a returned
ID to `map read --json` for `source_utf8`. Keep engine selection and rendering in
the engine. Pass opaque continuation cursors back unchanged rather than slicing
pack text. [Named context](https://github.com/Eris-Margeta/dctx/blob/main/docs/CONTEXT.md) adds stable chunk IDs and opt-in delivery
sessions: use `dctx context TARGET --session ID --dedupe`, or the existing MCP
`read` tool with `context: true`. Delivery history lasts for the CLI invocation
or MCP/RPC connection; general receipts remain planned. A Rust facade `Session`
is an in-process API object. See the [scope contract](#two-scopes-one-binary).

### Team / enterprise admin

Team / enterprise admin: system binary via package manager, signature + SBOM verification, one pinned version fleet-wide. Pitfall: mixing project and system versions without checking (`dctx doctor`).

This is the rollout target. Today, pin a release archive and check its matching
`SHA256SUMS` entry; package-manager distribution, signed CLI binaries and a
release SBOM verification workflow are planned. Deploy the verified archive and
root ignore rules through your existing fleet tooling. Record the resolved
binary path and start with:

```sh
dctx --version --verbose
```

Expect version and feature/precision limits. `dctx doctor` lists the dctx
binaries visible from a directory with their scopes and SHA-256 checksums;
signature-status output is planned. Check paths and versions explicitly during onboarding;
use [release procedure](https://www.dctx.internet.technology/docs/releasing/), [testing](https://github.com/Eris-Margeta/dctx/blob/main/docs/TESTING.md) and
[security reporting](https://www.dctx.internet.technology/security/) for the current controls. Signed
organization policy bundles and audit journals are planned.

## Safe path in ten minutes

### 1. Install the system binary

Download the archive for your OS and architecture and `SHA256SUMS` from
[GitHub Releases](https://github.com/Eris-Margeta/dctx/releases/tag/v1.3.1).
Compare the archive's SHA-256 with its matching entry before extracting it.
On Linux use `sha256sum ARCHIVE`, on macOS `shasum -a 256 ARCHIVE`, or on Windows
PowerShell `Get-FileHash ARCHIVE -Algorithm SHA256`, replacing `ARCHIVE` with the
downloaded filename. This checks integrity against the release checksums;
signed binary verification is planned.

Extract the archive and put `dctx` (`dctx.exe` on Windows) on your user `PATH`.
Archives cover Linux x64/ARM64, macOS x64/ARM64 and Windows x64.

```sh
dctx --version
```

Use the system binary for interactive agents. Project wrappers are available
through npm for Node and CI artifacts for Python and Go; no project dependency
is needed for this path.

### 2. Choose one project and inspect one answer

Use a small project or a relevant sub-root. Check `.gitignore` and `.dctxignore`
before scanning; leave `DCTX_INDEX_IGNORED` unset. From that directory:

```sh
dctx map find Config --json --budget 500 --limit 5
```

Replace `Config` with a symbol or heading in your project. Read a returned ID
with `dctx map read 'ID' --json`. The JSON includes verified source bytes as
`source_utf8` and reports precision, freshness and pagination. Keep the same
root between calls, because root-relative IDs change when the root changes.

### 3. Configure the lean MCP profile

In a client accepting `mcpServers`, replace both absolute paths:

```json
{
  "mcpServers": {
    "dctx": {
      "command": "/absolute/path/to/dctx",
      "args": ["mcp", "--profile", "lean", "/absolute/path/to/project"]
    }
  }
}
```

Restart the client and confirm that `find` and `read` are available. The lean
profile has **two tools**, not the planned `context/read/find` profile. For a
controlled comparison, turn off overlapping host file tools where the client
allows it; keep instructions and task scope consistent. Use the
[MCP guide](https://github.com/Eris-Margeta/dctx/blob/main/docs/guides/MCP.md) for profile selection and measured startup overhead,
and [client-specific configs](https://www.dctx.internet.technology/docs/modalities/#client-configuration) for other formats.

### 4. Measure before changing anything

Run the same small task with grep plus narrow file reads and with lean MCP.
Record task correctness, number of tool calls, startup instructions and schemas,
all model input/output tokens, provider cache hits, latency and billed cost when
available. Include the history carried into each model turn. Smaller tool
answers alone do not establish whole-session savings.

General receipts are planned. Today, retain JSON metadata and the agent's usage
report. For task packs, the shipped footer is a useful output accounting line:

```sh
dctx pack . --for 'Config' --budget 800
```

It reports `body_tokens`, `budget` and `dropped`, using estimated tokens rather
than a provider tokenizer. It is not a whole-session receipt. Save outputs
outside the selected tree, and compare repeated runs with identical inputs.
See [measurement evidence](https://www.dctx.internet.technology/docs/benchmark/) and
[CI/cache guidance](https://www.dctx.internet.technology/docs/modalities/#ci--prompt-cache-prefix-stability).

## Responsibilities

This table uses the scope contract's wording, with unshipped provisions marked
`(planned)`. Prebuilt archives with `SHA256SUMS` exist today; signed CLI binary
releases do not. ACP setup is planned; the MCP lean profile and its startup
measurement exist.

| Level | dctx provides | You are responsible for |
|---|---|---|
| System binary | Release archives with cosign signatures, SBOM and provenance, cache caps, `dctx doctor`, journals (planned), receipts (planned) | Choosing roots, policy file, keeping one version on PATH |
| Project library | Pinned vendored binary and protocol client (Node on npm; Python and Go from CI), same bytes as CLI (checked in CI) | Pinning the version, exclusions in the project policy, not re-implementing engine logic |
| MCP and ACP agent setup | Lean profile, tested client configs, measured start-up cost | Turning off the host's own file tools, picking a profile, session ids, reading receipts |
| CI and cache | Byte-stable pack, prefix tiers (planned), deterministic exit codes | Pinning the binary, caching the index between runs, treating pack drift as a signal not noise |

For current CLI exclusions, use `.gitignore` and `.dctxignore`; the unified policy
file is planned. Agent session ids and general receipts in the responsibility
column describe planned integrations. The Rust facade exists today with explicit
host-supplied selection and storage policy; the project-library row describes
the Python, Node and Go wrappers, with Node available on npm. Existing pack prefix stability is distinct
from the planned layered prefix tiers.

## Two scopes, one binary

The following is the distribution contract for the project-library wrappers
and signed releases:

> The system `dctx` on PATH belongs to you and your agents. A binary installed by a project library lives inside that project (virtualenv, node_modules), pinned and managed by that package manager. Same signed engine, different owner. Interactive agents and MCP call the system binary; application code and CI call the project binary; a library never silently falls back to PATH.

Today, install the system binary from release archives. The Python, Node and Go
wrappers already follow the no-fallback contract: they resolve an explicit path,
`DCTX_BIN`, their own platform package (Python, Node) or a versioned user cache,
and never search `PATH`. Until they are published, install them from CI
artifacts or a checkout, or invoke an explicitly pinned CLI path. The Rust facade
is an in-process library, not a vendored executable.

The CLI cache lives in `<cache-parent>/<engine-major.minor>/` under the
platform-specific `dctx/v2` parent (or `DCTX_CACHE_DIR`), so engines 1.2 and 1.3
keep separate indexes while patch releases of one family share theirs (see
[scopes](https://github.com/Eris-Margeta/dctx/blob/main/docs/SCOPES.md)). A version/path line in every pack is planned. Today
`dctx rpc` reports the engine version in `hello`, and `dctx doctor` lists the dctx
binaries visible from a directory. Check the executable path and version
explicitly, and use separate absolute `DCTX_CACHE_DIR` locations when two
installations of the same engine family must not share an index. Both future scopes should read the project's exclusions; today the
CLI loads ignore files while a Rust host supplies its own selection rules.

Continue with the [documentation index](https://github.com/Eris-Margeta/dctx/blob/main/docs/README.md), [modalities](https://www.dctx.internet.technology/docs/modalities/)
or [engine specification](https://www.dctx.internet.technology/docs/spec/).
