#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
set -euo pipefail
umask 022
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
export GOBIN="$root/target/actionlint/bin"
export GOCACHE="$root/target/actionlint/cache"
export GOMODCACHE="$root/target/actionlint/mod"
mkdir -p "$GOBIN"
# v1.7.7; resolved through api.github.com/repos/rhysd/actionlint/commits/v1.7.7.
go install github.com/rhysd/actionlint/cmd/actionlint@03d0035246f3e81f36aed592ffb4bebf33a03106
# Exact hosted-runner warning only; every other finding remains fatal.
"$GOBIN/actionlint" -ignore 'label "macos-15-intel" is unknown' .github/workflows/*.yml
