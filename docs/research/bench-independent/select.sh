#!/bin/sh
# Seeded repo selection (v1 card 6). usage: ./select.sh [--seed HEX | --seed-ref v1.0.0 --repo <dctx git dir>] [--no-resolve] [--out F]
# Default: seed = the commit of tag v1.0.0 in this dctx checkout; tags resolved with git ls-remote (network).
set -eu
exec python3 -I "$(cd "$(dirname "$0")" && pwd)/pick_repos.py" "$@"
