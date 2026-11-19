<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Releasing dctx

The current CLI packaging, registry publication, website deployment, secret setup
and rollback runbook is [docs/RELEASING.md](docs/RELEASING.md).

The desktop app retains its separate `desktop-v*` release workflow.

## Offline release preflight

Run `scripts/release-check.sh --preflight-only` with Python 3.11 or newer.
Full and quick release checks also run this preflight before building or using
the CLI binary. It checks the Cargo, npm and Python release versions against
`VERSION` and requires a matching release heading in `CHANGELOG.md`.

Set `rust-toolchain.toml`'s channel to the default CI toolchain. The preflight
reads `DCTX_CI_RUST` from `tools/release/toolchain.env` when referenced by a
workflow, and checks literal default toolchain declarations. The MSRV,
release-only and performance pins can differ. When workflows use commit SHA
pins, every external repository action and reusable workflow must use a full
commit SHA. Each check prints a PASS or FAIL line, and any failure blocks the
release check.

## Publication gate

Before any future production/public release, complete the required production
security review and full relevant audit for the exact candidate
revision and configuration, including independent review, validators and required
sandbox/runtime evidence. The focused DCX-03 static sweep and ordinary CI do not
satisfy this prerequisite. Keep findings, residual risks and coverage limits private.

Follow the owner policy: select an explicit
`<project-root>/.private/security-audits/<release-id>/run-<N>`
evidence directory, verify the entire tree is ignored before writing in a Git
project, and use owner-only permissions (exclude it from staging/export for a
non-Git project). Use the separate overlay instead of the vendor default path.
Any unresolved gate blocker needs explicit informed owner disposition tied to the
exact release. A gate pass is not a claim of complete security.

The owner must authorize the production tag/publication and deployment.
