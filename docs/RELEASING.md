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
   matching changelog section. Five native builds produce Linux x64/ARM64 **musl**,
   macOS x64/ARM64 and Windows x64 archives. Each contains `dctx` (`dctx.exe` on
   Windows), LICENSE and NOTICE. `release` creates SHA256SUMS and a GitHub Release
   whose notes are exactly that changelog section. Only this job has `contents: write`.
   Afterward `npm-publish` checksums the released assets, fills `packaging/npm/` from
   the tag, publishes five `@dctx/cli-*` packages before `dctx` with provenance and
   public access; `crates-publish` publishes in dependency order and Cargo waits for
   indexing. Each publishing job is skipped if its token is absent. Smoke-check
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

`GITHUB_TOKEN` is supplied automatically. npm alone gets `id-token: write` for
[provenance](https://docs.npmjs.com/generating-provenance-statements/). Missing any
site secret skips the deploy job. No registry tokens or SSH material belong in git.

**Rollback:** stop publication/deployment by removing its secrets or disabling the
workflow. Revert a bad website source change on main and regenerate the site to deploy
known-good bytes. For a bad CLI release, mark the GitHub Release prerelease/remove
its assets, deprecate affected npm versions and point `latest` to a known-good version;
yank affected crate versions (`cargo yank --version X.Y.Z CRATE`), then ship a new
patch version. Published versions cannot be overwritten. After a partial publish,
finish only the missing versions in dependency/platform-first order from the original
reviewed tag; do not rerun already successful immutable uploads blindly.
