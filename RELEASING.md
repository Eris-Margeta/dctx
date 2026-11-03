# Releasing the CLI

This checklist prepares a future CLI release. Version changes, tags, release
workflow execution, publication and deployment require separate owner authorization.

## Version plan

The candidate is **1.1.0**; the last published release is 1.0.0. The sync helper accepts stable `X.Y.Z` versions
only; prerelease version synchronization needs a separate change. Version lines:

| Scope | Current | Next release action |
| --- | --- | --- |
| `VERSION`, `project-meta.yaml`, README badge | 1.1.0 | Set `VERSION`, then run the sync helper |
| Every `crates/*` and `plugins/*` package and its `Cargo.lock` entry | 1.1.0 | Written by the sync helper from `VERSION` |
| Desktop Cargo package, package.json and tauri.conf.json | 0.11.0 | Retain; desktop has a separate `desktop-v*` workflow |

Do not bulk-replace dependency versions or nested test fixture versions. The
root workspace has no inherited package version. `dctx --version` prints the
`dctx-cli` package version and the short git sha embedded by `crates/dctx-cli/build.rs`
(`unknown` when built without git); it does not read `VERSION` at runtime.

In a separately authorized version-change task, set `VERSION`, then run these
commands from the repository root with zsh and Python 3 available:

```zsh
zsh tools/sync-version.sh --write
zsh tools/sync-version.sh --check
```

Both helpers use portable shell syntax and also run under zsh; hosted Ubuntu
release steps invoke their shebangs without requiring a zsh installation.
The default mode is `--check` and changes nothing. `--write` updates the metadata,
the numeric README badge version, the `[package]` version of every `crates/*` and
`plugins/*` manifest, and those packages' `version` lines in `Cargo.lock`. It does
not change `VERSION`, desktop files, dependency constraints or changelog headings.
Use this explicit `--write` procedure instead of the old no-argument write
instruction in `docs/ADR-006-Versioning-Strategy.md:32`; the unified desktop
inventory in `docs/ADR-013-Monorepo-Structure.md:265` is historical.

Refresh the lockfile on the remote builder after changing package versions:

```zsh
/path/to/bin/remote.sh build-host --src "$PWD" --name dctx-DCX-05 --pull Cargo.lock -- cargo check -p dctx-cli
git diff -- Cargo.lock
```

Review every lockfile change; do not accept unrelated dependency upgrades. For
remote infrastructure exit 255 or 75, retry once on build-host, then use `mint` with
the same arguments. Never run Cargo directly on this Mac.

Move completed Unreleased entries into `## [X.Y.Z] — YYYY-MM-DD` using the actual
release date, and open a fresh `## [Unreleased]` section above it when work resumes. The extractor
reads root `CHANGELOG.md`, matches bracketed versions literally, skips Unreleased
by default, accepts an optional `v` prefix and fails for a missing version.

## Preflight for the exact candidate

Record the candidate SHA and inspect the complete worktree and index first.
Resolve ownership of unrelated changes before preparing a clean release candidate.
Do not stash, stage or discard another worker's files.

```zsh
git branch --show-current                 # must be main
git status --short
git diff --cached --name-only
git rev-parse HEAD
test -s RELEASING.md
git diff --check
zsh -n tools/extract-changelog.sh
zsh -n tools/sync-version.sh
zsh tools/sync-version.sh --check
zsh tools/extract-changelog.sh "$(cat VERSION)"  # must be the dated candidate entry
ruby -r yaml -e 'w = YAML.load_file(".github/workflows/release.yml"); abort "missing matrix" unless w.dig("jobs", "build", "strategy", "matrix", "include").size == 4'
/path/to/bin/remote.sh build-host --src "$PWD" --name dctx-DCX-05 -- cargo build --locked --release -p dctx-cli --bin dctx-cli --target x86_64-unknown-linux-gnu
```

The Ruby command parses YAML; also inspect every matrix tuple, target installation,
build/smoke path, archive member and upload path against the table below. A YAML
parse does not prove GitHub Actions expression semantics or cross-platform builds.
Require successful main CI for the exact candidate, not a previous commit:

```zsh
gh run list --workflow ci.yml --branch main --commit "$(git rev-parse HEAD)" --json databaseId,headSha,status,conclusion,url
gh run view <matching-run-id> --json headSha,status,conclusion,jobs,url
```

Check that `headSha` matches and all required jobs conclude `success`. The normal
CI runs Linux Clippy/tests and excludes desktop; it does not validate this release
matrix. Record native target build, `--version` smoke and archive inspection
receipts separately before claiming a platform is release-ready. This checklist
does not dispatch the tag-only workflow to obtain those receipts.

## CLI matrix and archive contract

| Cargo target | Native runner | Archive | Executable inside |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `ubuntu-24.04` | `dctx-linux-x64.tar.gz` | `dctx` |
| `x86_64-apple-darwin` | `macos-15-intel` | `dctx-macos-x64.tar.gz` | `dctx` |
| `aarch64-apple-darwin` | `macos-15` (ARM64) | `dctx-macos-arm64.tar.gz` | `dctx` |
| `x86_64-pc-windows-msvc` | `windows-2022` | `dctx-windows-x64.zip` | `dctx.exe` |

Runner architecture labels were checked against the [GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners);
recheck availability before a future release. Cargo's implicit binary target is
`dctx-cli` (`src/main.rs`); it builds to
`target/<target>/release/dctx-cli[.exe]`. Packaging renames only the archived copy
and includes root `LICENSE` and `NOTICE`. Uploads fail when the expected archive
is missing. All builds use the committed lockfile and default CLI features.

These are four planned CLI artifacts, not a verified portability promise. There
is no Linux ARM64/musl, Windows ARM64, universal macOS binary, desktop bundle,
code signing, notarization, checksum manifest, SBOM or provenance attestation in
this workflow. GNU Linux builds inherit the runner's libc baseline; test on the
oldest intended deployment environment. Plugin runtime/confinement availability
must be assessed independently of successful compilation and `--version`.

## Publication gate

Before any future production/public release, load the production security gate
and `security-audit` and complete the full relevant audit for the exact candidate
revision and configuration, including independent review, validators and required
sandbox/runtime evidence. The focused DCX-03 static sweep and ordinary CI do not
satisfy this prerequisite. Keep findings, residual risks and coverage limits private.

Follow the owner policy: select an explicit
`/path/to/<project-root>/.private/security-audits/<release-id>/run-<N>`
evidence directory, verify the entire tree is ignored before writing in a Git
project, and use owner-only permissions (exclude it from staging/export for a
non-Git project). Use the separate overlay instead of the vendor default path.
Any unresolved gate blocker needs explicit informed owner disposition tied to the
exact release. A gate pass is not a claim of complete security.

`release.yml` is triggered by `v*` tags and can create a non-draft GitHub release;
only its final job receives `contents: write`. Its metadata checks require the tag,
VERSION, CLI/core versions and numbered changelog entry to agree; verify the date
manually. Do not create or push a tag, execute this workflow, publish crates,
deploy or change visibility
until the owner separately authorizes those actions and the gate is satisfied.
