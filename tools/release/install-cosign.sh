#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
# Signing/verification jobs run on Linux x86_64. Pin both version and downloaded bytes.
set -euo pipefail
umask 022
root=$(cd "$(dirname "$0")/../.." && pwd)
destination="$root/target/cosign/bin"
mkdir -p "$destination"
temporary=$(mktemp -d "$root/target/cosign/download.XXXXXX")
trap 'rm -rf "$temporary"' EXIT
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location --retry 3 \
  https://github.com/sigstore/cosign/releases/download/v3.1.4/cosign-linux-amd64 \
  --output "$temporary/cosign"
# GitHub API release asset digest for sigstore/cosign v3.1.4, cosign-linux-amd64.
printf '%s  %s\n' '424e2c97d0b45fd7a6b608a87f090f35633f4c23ece5f092d48bba11aaf4bbfa' \
  "$temporary/cosign" | sha256sum --check
install -m755 "$temporary/cosign" "$destination/cosign"
"$destination/cosign" version
if [[ -n "${GITHUB_PATH:-}" ]]; then printf '%s\n' "$destination" >> "$GITHUB_PATH"; fi
