# Contributing to dctx

Read the [specification](https://www.dctx.internet.technology/docs/spec/), [crate map](https://www.dctx.internet.technology/docs/crate-map/) and
[Code of Conduct](https://github.com/Eris-Margeta/dctx/blob/main/CODE_OF_CONDUCT.md) before making changes. Project contributions
are licensed under [Apache-2.0](https://github.com/Eris-Margeta/dctx/blob/main/LICENSE), consistent with [NOTICE](https://github.com/Eris-Margeta/dctx/blob/main/NOTICE).

## Bugs, ideas and questions

Search existing [issues](https://github.com/Eris-Margeta/dctx/issues) before opening
one. Use the bug or feature template and keep reproductions small. Include the
commit or version, OS/architecture, build features, exact command, expected result
and actual result. Redact source code, paths, credentials and generated context
that you do not have permission to share.

Report suspected vulnerabilities privately using [SECURITY.md](https://www.dctx.internet.technology/security/),
rather than in an issue or pull request. For other questions, open an issue with
what you tried and the relevant documentation link.

## CLI development setup

Use a recent stable Rust toolchain with Cargo; the workspace includes Rust 2024
crates. Clone the repository and run commands from its root:

```zsh
git clone https://github.com/Eris-Margeta/dctx
cd dctx
cargo build --locked -p dctx-cli
cargo run --locked -p dctx-cli -- --help
```

Cargo builds **`dctx-cli`**, while help text calls the tool **`dctx`**. See
[installation](https://www.dctx.internet.technology/docs/readme/#installation) for aliases and install choices.

Keep changes focused on the relevant crates and preserve unrelated work. The
[`dctx` facade](https://github.com/Eris-Margeta/dctx/tree/main/crates/dctx) serves the CLI and the desktop API; pure engine
crates should not acquire filesystem, network or process effects. The
[plugin guide](https://www.dctx.internet.technology/docs/plugins/) describes effect boundaries and current
runtime limitations.

## Validation

Run focused tests for changed behavior, then the relevant broader checks. Current
[main CI](https://github.com/Eris-Margeta/dctx/blob/main/.github/workflows/ci.yml) runs these Rust checks, excluding the desktop:

```zsh
cargo clippy --workspace --exclude dctx-desktop --all-targets -- -D warnings -A clippy::field_reassign_with_default -A clippy::assertions_on_constants
cargo nextest run --workspace --exclude dctx-desktop --no-fail-fast
```

The test command requires the `cargo-nextest` tool. CI closes inherited descriptors
before nextest because native confinement tests reject extra descriptors; the
workflow contains that wrapper. Preserve those tests' confinement checks.

Use `cargo fmt --all -- --check` to check formatting and document public APIs.
Desktop changes need their platform prerequisites and separate validation; see
[the desktop guide](https://github.com/Eris-Margeta/dctx/blob/main/apps/dctx-desktop/README.md). The root `just ci` recipe also
invokes desktop setup and is broader than the current main CI job.

## Developer Certificate of Origin

Contributions use the [Developer Certificate of Origin](https://developercertificate.org)
(DCO) instead of a contributor license agreement. Sign off each commit to certify that
you wrote the change, or otherwise have the right to submit it under Apache-2.0:

```zsh
git commit -s
```

The `-s` flag adds a `Signed-off-by:` line with your name and email. Pull requests with
unsigned commits will be asked to amend them with `git commit --amend -s`, or to rebase
with `--signoff`.

## Pull requests

Create a focused branch from `main` and describe the problem, resulting behavior
and any compatibility or privacy impact. Add regression coverage for behavior
changes and update affected documentation. Check Markdown links and paths when
editing guides. Use clear commit messages, for example `fix(cli): reject empty
output paths`.

In the PR, list the commands you ran, their results, and any checks you could not
run. Include relevant OS/build features for platform-specific changes. Avoid
unrelated formatting, generated governance files, local caches, credentials and
private research artifacts. Maintainers may ask for a smaller change or additional
validation before merging. Fork PRs do not run on the self-hosted CI runner;
maintainers must arrange an appropriate review and validation path.
