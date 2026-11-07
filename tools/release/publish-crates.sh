#!/usr/bin/env bash
# Publish the workspace's public crates; Cargo waits for each upload to be indexed.
set -euo pipefail
cd "$(dirname "$0")/../.."
exec python3 tools/release/publish-crates.py "$@"
