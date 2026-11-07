# Releasing dctx

The current CLI packaging, registry publication, website deployment, secret setup
and rollback runbook is [docs/RELEASING.md](https://github.com/Eris-Margeta/dctx/blob/main/docs/RELEASING.md).

The desktop app retains its separate `desktop-v*` release workflow.

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
