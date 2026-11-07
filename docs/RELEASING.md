<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Releasing dctx

Maintainer runbook for `.github/workflows/release.yml` and `deploy-site.yml`. Desktop
keeps its separate `desktop-v*` workflow. The existing [publication gate](../RELEASING.md#publication-gate)
still applies. Do not use `just release` without reviewing
its commit/tag/push prompts. Publication starts when a maintainer pushes a `v*` tag.

1. **Prepare:** set stable `X.Y.Z` in `VERSION`; run `tools/sync-version.sh --write`
   and `--check`. This updates crate versions and exact registry requirements,
   Cargo.lock and project metadata; desktop versions remain separate. Move completed
   changes into a dated `## [X.Y.Z]` section in root `CHANGELOG.md`.
2. **Validate:** run normal CI/release gates, `cargo package --list -p dctx-cli`, and
   `tools/release/publish-crates.sh --dry-run --allow-dirty`. First-release dry runs
   of dependent crates need their dependencies already indexed on crates.io; use
   `--offline` locally to diagnose cached availability. Dev-only path dependencies
   intentionally have no registry version and Cargo omits them from published manifests,
   avoiding dev dependency publication cycles. There are 36 public crates, including
   the bundled `dctx-plugin-git`; both desktop packages have `publish = false`.
3. **Tag:** after review, commit the version/changelog and push main, then
   `git tag -a "v$(cat VERSION)" -m "dctx $(cat VERSION)"` and
   `git push origin "v$(cat VERSION)"`. Never move an existing published tag.
4. **Observe:** preflight verifies VERSION, all crate versions/requirements and the
   matching changelog section. Five native builds use the Rust pin in
   `tools/release/toolchain.env` and `tools/release/build.sh` to produce Linux
   x64/ARM64 **musl**, macOS x64/ARM64 and Windows x64 archives, plus target-specific
   CycloneDX JSON SBOMs (`cargo-cyclonedx` 0.5.10, Cargo metadata, default CLI features).
   Each native archive contains `dctx` (`dctx.exe` on Windows), LICENSE and NOTICE.
   Two additional fresh-runner Linux x86_64 musl builds must produce identical
   SHA-256 digests and bytes before signing. This gate can also be run manually via
   `reproducible.yml` without publishing. See [verification and reproducibility](VERIFYING.md).
   The `sign` job alone has `id-token: write` and `attestations: write`. It uses
   GitHub OIDC with cosign 3.1.4, signs all archives/SBOMs/checksums/provenance
   envelopes and emits a `.sigstore.json` bundle per payload. It creates and signs
   per-platform air-gap tarballs, with directly installable binaries, internal
   signed manifests and verification evidence, and attests every native and air-gap
   archive using GitHub artifact attestations. Only `release` has `contents: write`;
   it publishes the sealed assets with the changelog notes. For a manual release
   dispatch, select the release tag as the workflow ref **and** supply that tag as
   the input (`gh workflow run release.yml --ref vX.Y.Z -f tag=vX.Y.Z`); preflight
   rejects a different ref so certificate identities always bind the release tag.
   Afterward `npm-publish` checksums the released assets and publishes five
   `@dctx/cli-*` packages before `dctx`. It uses the npm token without an OIDC grant;
   npm provenance is not emitted because OIDC is restricted to `sign`. The GitHub
   archives retain the documented cosign and GitHub attestation evidence.
   `crates-publish` publishes in dependency order and Cargo waits for indexing.
   Each registry job is skipped if its token is absent. Smoke-check
   `npm exec --package=dctx -- dctx --version` and `cargo install dctx-cli --version X.Y.Z`
   (the Cargo executable is `dctx-cli`; release archives/npm expose `dctx`).
5. **Website:** pushes to main affecting website, website tooling, docs or CHANGELOG
   build with the pinned requirements in a Python venv, then run
   `check.py --working-tree --no-capture`
   before rsync. The full CLI capture check remains in normal CI. Generated website
   files must be committed alongside source changes for normal CI (`just site`);
   deployment checks its fresh build against an independent rebuild. Update
   `tools/website/data/dates.json` with `build.py --refresh-dates` after doc commits.
   Deployment serializes runs and pins SSH host keys. The remote `./` resolves to
   `/srv/sites/dctx/` through the server's `rrsync -wo /srv/sites/dctx` forced command.

Create these **repository Actions secrets** before enabling publication/deployment:

| Secret | Required credentials |
| --- | --- |
| `NPM_TOKEN` | npm granular publish token with write access to `dctx` and all five `@dctx/cli-*` packages; allow CI publication with the account's 2FA policy. Create/own the `@dctx` scope and verify all names before the first tag. |
| `CARGO_REGISTRY_TOKEN` | crates.io API token permitted to create/publish all 36 names printed by `tools/release/publish-crates.sh --list`. Check availability beyond just `dctx` and `dctx-cli`. |
| `DCTX_SITE_SSH_KEY` | Dedicated OpenSSH private deploy key, no passphrase. The deploy administrator installs the public key with `restrict,command="rrsync -wo /srv/sites/dctx"`; that directory must be writable by the deploy account. |
| `DCTX_SITE_HOST` | SSH destination `deploy-user@hostname` (port 22), without a path. |
| `DCTX_SITE_KNOWN_HOSTS` | Complete OpenSSH known_hosts lines for that hostname, verified out of band; never discover/trust keys during a deploy. |

`GITHUB_TOKEN` is supplied automatically. Only the signing job gets OIDC access;
keyless signing needs no stored signing key. The cosign bootstrap is pinned by
version and the GitHub API release-asset SHA-256 in `tools/release/install-cosign.sh`. The repository must permit GitHub
artifact attestations. Enable GitHub private vulnerability reporting as described
in [SECURITY.md](../SECURITY.md). Missing any site secret skips the deploy job.
No registry tokens or SSH material belong in git.

The quality job runs pinned `cargo-audit` 0.22.2 against Cargo.lock alongside the
existing cargo-deny licence policy. `.cargo/audit.toml` contains the committed
advisory ignore list, initially empty. An exception requires an advisory ID,
affected dependency path, reason and expiry; do not add ignores merely to make
CI green. Audit needs current RustSec advisory data and fails if it cannot fetch
it. Workflow lint uses actionlint v1.7.7 at its API-resolved commit; only the known
`macos-15-intel` runner-label warning is ignored. Run
`bash tools/release/lint-workflows.sh` and
`python3 -m unittest discover -s tools/release -p 'test_supply_chain.py' -v` locally.
All third-party action references are full commit SHAs resolved through the GitHub
commits API. Update pins deliberately and validate workflows after each update.

**Rollback:** stop publication/deployment by removing its secrets or disabling the
workflow. Revert a bad website source change on main and regenerate the site to deploy
known-good bytes. For a bad CLI release, mark the GitHub Release prerelease/remove
its assets, deprecate affected npm versions and point `latest` to a known-good version;
yank affected crate versions (`cargo yank --version X.Y.Z CRATE`), then ship a new
patch version. Published versions cannot be overwritten. After a partial publish,
finish only the missing versions in dependency/platform-first order from the original
reviewed tag; do not rerun already successful immutable uploads blindly.
