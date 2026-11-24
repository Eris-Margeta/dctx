#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Verify published archives in the current directory, before extraction.
set -euo pipefail
export LC_ALL=C GH_HOST=github.com GH_PROMPT_DISABLED=1
export GH_NO_UPDATE_NOTIFIER=1 GH_NO_EXTENSION_UPDATE_NOTIFIER=1

if [[ $# != 4 ]]; then
    echo 'Usage: verify-offline-provenance.sh TAG TARGET COSIGN_TRUSTED_ROOT GH_TRUSTED_ROOT' >&2
    exit 2
fi
tag=$1
target=$2
cosign_root=$3
gh_root=$4
if [[ ! $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
    printf 'Invalid release tag %q\n' "$tag" >&2
    exit 2
fi
case $target in
    x86_64-unknown-linux-musl|aarch64-unknown-linux-musl|x86_64-apple-darwin|aarch64-apple-darwin)
        native="dctx-$target.tar.gz" ;;
    x86_64-pc-windows-msvc)
        native="dctx-$target.zip" ;;
    *)
        printf 'Unknown release target %q\n' "$target" >&2
        exit 2 ;;
esac
outer="dctx-$target-airgap.tar.gz"
repo=Eris-Margeta/dctx
workflow="$repo/.github/workflows/release.yml"
ref="refs/tags/$tag"
identity="https://github.com/$workflow@$ref"
issuer=https://token.actions.githubusercontent.com

for file in "$cosign_root" "$gh_root" "$outer" "$outer.sigstore.json" airgap-provenance.jsonl \
            "$native" "$native.sigstore.json" archive-provenance.jsonl; do
    if [[ ! -f $file || ! -r $file || ! -s $file ]]; then
        printf 'Missing, empty or unreadable verification input %q\n' "$file" >&2
        exit 1
    fi
done

require_options() {
    local help_text=$1 option
    shift
    for option in "$@"; do
        if [[ ! $help_text =~ (^|[[:space:],])$option([[:space:],=]|$) ]]; then
            printf 'Verifier lacks required option %q\n' "$option" >&2
            exit 1
        fi
    done
}

if ! help_text=$(cosign verify-blob --help 2>/dev/null); then
    echo 'Cannot inspect cosign verify-blob' >&2
    exit 1
fi
require_options "$help_text" --trusted-root --bundle --certificate-identity --certificate-oidc-issuer
if ! help_text=$(gh attestation verify --help 2>/dev/null); then
    echo 'Cannot inspect gh attestation verify' >&2
    exit 1
fi
require_options "$help_text" --bundle --custom-trusted-root --repo --signer-workflow \
    --source-ref --deny-self-hosted-runners --predicate-type

verify_archive() {
    local archive=$1 provenance=$2
    # Verifier output can include arbitrary certificate, bundle or filename text.
    if ! cosign verify-blob --trusted-root "$cosign_root" --bundle "$archive.sigstore.json" \
        --certificate-identity "$identity" --certificate-oidc-issuer "$issuer" \
        "$archive" >/dev/null 2>&1; then
        printf 'cosign refused blob %q\n' "$archive" >&2
        exit 1
    fi
    if ! gh attestation verify "$archive" --bundle "$provenance" --custom-trusted-root "$gh_root" \
        --repo "$repo" --signer-workflow "$workflow" --source-ref "$ref" \
        --deny-self-hosted-runners --predicate-type https://slsa.dev/provenance/v1 >/dev/null 2>&1; then
        printf 'gh refused provenance %q for %q\n' "$provenance" "$archive" >&2
        exit 1
    fi
}

verify_archive "$outer" airgap-provenance.jsonl
verify_archive "$native" archive-provenance.jsonl
printf 'Verified release signatures and provenance for %q and %q at %q\n' "$outer" "$native" "$tag"
