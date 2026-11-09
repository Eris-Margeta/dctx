<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Verifying and installing CLI releases

These instructions apply to CLI releases made with the supply-chain workflow in
`.github/workflows/release.yml`. Earlier releases and the separately versioned
`desktop-v*` preview do not retroactively gain these signatures or attestations.
Use the `dctx` binary as the source of truth for available commands and features.

## Verify before extraction

Install [cosign 3.1.4](https://github.com/sigstore/cosign/releases/tag/v3.1.4)
using Sigstore's published verification instructions (or the version/digest-pinned
`tools/release/install-cosign.sh` on Linux x86_64) and a recent GitHub CLI with
`gh attestation` support. The workflow pins that cosign version. Select a release
tag and platform explicitly; the examples use Linux x86_64 musl. ARM64 Linux,
Intel/Apple Silicon macOS and Windows x64 have corresponding assets.

```sh
TAG=v1.3.0 # select a release produced by the new workflow
ASSET=dctx-x86_64-unknown-linux-musl.tar.gz
REPO=Eris-Margeta/dctx
gh release download "$TAG" --repo "$REPO" --pattern "$ASSET" \
  --pattern "$ASSET.sigstore.json" --pattern SHA256SUMS \
  --pattern SHA256SUMS.sigstore.json

cosign verify-blob \
  --bundle "$ASSET.sigstore.json" \
  --certificate-identity "https://github.com/Eris-Margeta/dctx/.github/workflows/release.yml@refs/tags/$TAG" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  "$ASSET"

cosign verify-blob \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity "https://github.com/Eris-Margeta/dctx/.github/workflows/release.yml@refs/tags/$TAG" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
# Check just the selected asset; the manifest also lists assets you did not download.
awk -v name="$ASSET" '$2 == name { print; found=1 } END { if (!found) exit 1 }' SHA256SUMS > selected.sha256
sha256sum --check selected.sha256
```

Require every command to succeed. Choose the expected tag yourself: accepting any
workflow identity or trusting a checksum downloaded beside a binary is insufficient.
Each payload has a `.sigstore.json` bundle containing its signature, short-lived
Fulcio certificate and Rekor transparency evidence. Bundles are verification
material, not additional recursively signed payloads. The top-level signed
`SHA256SUMS` covers all native archives, per-target CycloneDX JSON SBOMs, air-gap
archives and provenance envelopes. Verify a `.cdx.json` or `-airgap.tar.gz` by
setting `ASSET` to that filename and using the same command.

On macOS use `shasum -a 256 -c selected.sha256` if `sha256sum` is unavailable.
On Windows run the example in Git Bash with cosign and `sha256sum` on PATH, or
compare `Get-FileHash -Algorithm SHA256` against the authenticated manifest.

## Verify GitHub build provenance

Every native archive and air-gap archive has a GitHub artifact attestation. Verify
repository, signer workflow and tag before using the bytes:

```sh
gh attestation verify "$ASSET" --repo Eris-Margeta/dctx \
  --signer-workflow Eris-Margeta/dctx/.github/workflows/release.yml \
  --source-ref "refs/tags/$TAG" --deny-self-hosted-runners
```

The signing job attests the archives built by the same tag-triggered release
workflow; it then creates and attests the air-gap archives. The release also
publishes `archive-provenance.jsonl` and `airgap-provenance.jsonl`, which allow
verification with `--bundle` without fetching attestations from GitHub. Provenance
binds bytes to a workflow and source; it is not a claim of independent review or
of reproducibility on every platform.

## Air-gap transfer and offline install

Each `dctx-TARGET-airgap.tar.gz` contains the executable, LICENSE, NOTICE, platform
SBOM, the original signed native archive, its cosign bundle, the SBOM bundle,
`archive-provenance.jsonl`, this guide, and an internal `SHA256SUMS` with its own
cosign bundle. The original archive is retained so provenance and its signature
can be verified without repacking the executable.

Before disconnecting, obtain a trusted cosign executable and authenticate current
Sigstore trust material independently of the dctx release. With cosign 3.1.4:

```sh
# cosign initialize authenticates TUF metadata using its embedded trust root.
export TUF_ROOT="$PWD/sigstore-cache"
cosign initialize
# The initialized cache contains the authenticated trusted_root.json target.
find "$TUF_ROOT" -type f -name trusted_root.json
# Copy the target printed above to your transfer media as trusted_root.json.
```

Transfer `trusted_root.json`, cosign, the selected air-gap tarball and its external
`.sigstore.json` bundle through your approved channel. Record the expected tag
and workflow identity separately. Retain the authenticated trust root for later
verification; signatures use signing-time transparency evidence even after their
short-lived certificates expire. The dctx bundle never supplies a new trust anchor.

Cosign 3.1.4 verifies bundled transparency evidence locally; an explicit
`--trusted-root` avoids TUF network access. It has no `--offline` flag. Do not
substitute the older cosign 2.x flags for the version used here.

On the disconnected host (Linux example), with an absolute path to the trusted root:

```sh
set -eu
TAG=v1.3.0 # the expected release tag
ASSET=dctx-x86_64-unknown-linux-musl-airgap.tar.gz
TRUSTED_ROOT=/media/transfer/trusted_root.json
IDENTITY="https://github.com/Eris-Margeta/dctx/.github/workflows/release.yml@refs/tags/$TAG"

cosign verify-blob --trusted-root "$TRUSTED_ROOT" \
  --bundle "$ASSET.sigstore.json" --certificate-identity "$IDENTITY" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com "$ASSET"
mkdir verified-dctx
tar -xzf "$ASSET" -C verified-dctx
cd verified-dctx
cosign verify-blob --trusted-root "$TRUSTED_ROOT" \
  --bundle SHA256SUMS.sigstore.json --certificate-identity "$IDENTITY" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS
sha256sum --check SHA256SUMS
install -Dm755 dctx "$HOME/.local/bin/dctx"
"$HOME/.local/bin/dctx" --version
```

Add `$HOME/.local/bin` to PATH. No Cargo, registry, network service or installer is
needed on the offline machine. On macOS use `mkdir -p "$HOME/.local/bin"` followed
by `install -m755 dctx "$HOME/.local/bin/dctx"`; use `shasum -a 256 -c SHA256SUMS`.
On Windows extract the verified tarball with `tar`, validate the signed internal
manifest and its hashes, and copy `dctx.exe` to a directory on your user PATH.
Do not run an executable until signature and checksum verification succeed. Never
skip certificate identity, issuer, transparency or certificate verification to make
an offline check pass.

## Rebuilding and reproducibility limits

Release Rust is pinned to **1.99.0** in `tools/release/toolchain.env`, independently
of the workspace MSRV of 1.91. Builds use the committed Cargo.lock, default CLI
features, `--locked --release -p dctx-cli --bin dctx-cli --target TARGET`,
`CARGO_INCREMENTAL=0`, `RUSTC_WRAPPER=`, `TZ=UTC`, `LC_ALL=C`, and umask 022.
`SOURCE_DATE_EPOCH` is the tagged commit's Unix commit timestamp. `DCTX_GIT_SHA`
is the tagged commit ID. Source, Cargo home and target paths are remapped to
`/dctx`, `/cargo` and `/target` with `--remap-path-prefix`, and
`-C debuginfo=0` removes host debug paths. C compiler path remapping is set for
bundled SQLite and future native dependencies. Linux musl requires `musl-tools`
for its C compiler/headers and uses Rust's self-contained CRT and
`rust-lld`, with `-C link-self-contained=yes -C link-arg=--build-id=none`.
Windows uses MSVC `/pathmap` for C paths and linker `/Brepro` to avoid wall-clock
PE timestamps. These flags reduce variation without asserting Windows reproducibility.

Tar entries are sorted, regular files with modes 0755/0644, uid/gid 0, empty owner
names and commit-time mtimes. USTAR avoids platform-specific extended headers;
gzip uses no filename and mtime 0. ZIP uses sorted entries, fixed DOS timestamps,
Unix modes and no compression. CI pins archive Python to 3.12.8. Run the same recipe:

```sh
sudo apt-get install musl-tools # Linux: C compiler/headers for bundled SQLite
source tools/release/toolchain.env
rustup toolchain install "$DCTX_RELEASE_RUST" --profile minimal --target x86_64-unknown-linux-musl
bash tools/release/build.sh x86_64-unknown-linux-musl dist
```

The `Reproducible Linux release archive` workflow is callable manually with a ref,
and is required by the release workflow on every `v*` tag. It resolves the ref once,
builds in two separate fresh GitHub-hosted runners with distinct checkout paths,
Cargo homes and target directories and no restored build cache, then compares
SHA-256 and bytes. The signing job also requires the actual release archive's
SHA-256 to match that result. A mismatch prevents signing/publication; hashes are
retained in the workflow summary and `reproducibility-report` artifact. Manual check:

```sh
gh workflow run reproducible.yml --repo Eris-Margeta/dctx -f ref="$TAG"
```

The gate covers the Linux x86_64 musl native archive only. Hosted runner images,
musl headers/GCC and the native SQLite build, host linkers/SDKs on macOS and Windows, and zlib versions can change independently
of our Rust/Python pins; cross-image/cross-platform reproducibility is not promised.
Future build scripts that embed time, entropy, absolute paths or native tool output
can break it and must be investigated rather than waived. SBOMs may contain UUIDs
and metadata timestamps; cosign certificates, transparency entries and provenance
are deliberately run-specific. Air-gap archives embed that evidence and therefore
are not expected to reproduce byte for byte across releases or signing runs.
