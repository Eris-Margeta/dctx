#!/bin/sh
# DX-A1 baseline runner for the current dctx map mode.
# Implements ARCHITECTURE-PROPOSAL §3 and §7 Stage 0.
# Depends on cargo and the map crates; the binary stays in scratch outside the repo.
# Used by the DX-A1 packet and tools/architecture-bench/README.md.

set -eu

cd "$(dirname "$0")/../.."
ROOT=$(pwd)
BENCH="$ROOT/tools/architecture-bench"
TMP="${TMPDIR:-/tmp}/dx-a1-architecture-bench"
mkdir -p "$TMP" "$BENCH/reports"
export CARGO_TARGET_DIR="$TMP/target"

cargo build --manifest-path "$BENCH/Cargo.toml" --release

BIN="$CARGO_TARGET_DIR/release/architecture-bench"
if [ ! -x "$BIN" ]; then
  echo "architecture-bench binary missing: $BIN" >&2
  exit 1
fi

"$BIN" \
  --repo "$ROOT" \
  --scratch "$BENCH/scratch" \
  --reports "$BENCH/reports" \
  --cache "$TMP/cache" \
  --manifest "$BENCH/corpus/manifest.json"
