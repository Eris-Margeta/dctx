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

## Disconnected verification before installation

On the disconnected machine, run the [consumer verification command](tools/release/verify-offline-provenance.sh)
from a directory containing the published files for one tag and target. Run it
before extracting either archive or copying an executable. It reads both archives
in place. For each one it checks the cosign blob signature and the SLSA build
provenance against that archive's bytes.

| Provision independently of the release payloads | Transfer from the selected release |
| --- | --- |
| Bash, an authenticated copy of the consumer command, [cosign 3.1.4](https://github.com/sigstore/cosign/releases/tag/v3.1.4) and [GitHub CLI with offline attestation verification](https://cli.github.com/manual/gh_attestation_verify) on a trusted PATH | The native `dctx-TARGET.tar.gz` (Windows uses `dctx-TARGET.zip`), its `.sigstore.json` sidecar and `archive-provenance.jsonl` |
| An authenticated Sigstore `trusted_root.json` for cosign and the independently authenticated [trusted root material for gh](https://cli.github.com/manual/gh_attestation_trusted-root) appropriate to the signing infrastructure | `dctx-TARGET-airgap.tar.gz`, its `.sigstore.json` sidecar and the external `airgap-provenance.jsonl` |

MAIN can replay the following from the transfer directory with networking
disabled. In this example, `../consumer` holds the trusted script and `../trust`
holds the two preprovisioned root files. Set `TAG` and `TARGET` from the
independently recorded release selection. Use a directory under your control
and retain the same verified bytes through installation.

```sh
set -eu
TAG=v1.3.2
TARGET=x86_64-unknown-linux-musl
bash ../consumer/verify-offline-provenance.sh "$TAG" "$TARGET" \
  ../trust/trusted_root.json ../trust/gh-trusted-root.jsonl
```

Repeat with `TARGET=x86_64-pc-windows-msvc` and the corresponding genuine
assets to cover ZIP. A zero exit requires both verifiers to accept both archives.
The cosign certificate must name the release workflow at the exact tag with the
GitHub Actions OIDC issuer. The GitHub attestation must match
`Eris-Margeta/dctx`, `.github/workflows/release.yml` and `refs/tags/$TAG`,
and pass the hosted-runner policy. A missing input, unsupported option or
verifier refusal stops the procedure. After success, continue with the signed
manifest and checksum checks in the [offline install guide](docs/VERIFYING.md#air-gap-transfer-and-offline-install).

`python3 -B tools/release/test_offline_provenance.py` runs policy and ordering
tests with recording verifiers. Only a disconnected run with genuine signed
material, independently trusted roots and the real verifier binaries provides
cryptographic evidence. Record the tag, target, verifier versions, independently
authenticated root identifiers and command exit status for that run.

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
