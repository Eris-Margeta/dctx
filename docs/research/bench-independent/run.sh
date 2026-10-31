#!/bin/sh
# Independent benchmark (v1 card 6), one command on the Linux runner:
#   ./run.sh <dctx-binary>            seed = commit of tag v1.0.0 in this checkout (INDEP_SEED=<sha> overrides, recorded)
# Steps: select.sh (seeded repos) -> setup.sh (only network step) -> gold_lsp.py (LSP gold) -> questions.py (20 questions)
# -> run_indep.py (dctx, Trail-Code, graphify, isolated) -> report.py. Prints the table, last line INDEP-BENCH DONE <dir>.
# The table is written whatever the numbers are; a non-zero exit means the harness broke, never that a tool scored low.
set -eu
[ $# -ge 1 ] || { echo "usage: $0 <dctx-binary>" >&2; exit 2; }
DCTX=$1
[ -x "$DCTX" ] || { echo "not executable: $DCTX" >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
export B3_ROOT=${B3_ROOT:-/path/to/workdir/bench/dctx3}
export B3_DCTX=$(readlink -f "$DCTX")
export B3_RESULTS="$HERE/results/$(date -u +%Y%m%dT%H%M%SZ)"
export TIKTOKEN_CACHE_DIR=$B3_ROOT/tiktoken-cache
mkdir -p "$B3_RESULTS/gold" "$B3_ROOT/tmp" "$B3_ROOT/cache" "$B3_ROOT/work"
SEL=$B3_RESULTS/selection.json
if [ -n "${INDEP_SEED:-}" ]; then sh "$HERE/select.sh" --seed "$INDEP_SEED" --out "$SEL"; else sh "$HERE/select.sh" --out "$SEL"; fi
R=$B3_RESULTS
step() { log=$R/$1; shift; "$@" > "$log" 2>&1 || { tail -30 "$log" >&2; echo "step failed, log: $log" >&2; exit 3; }; }
step setup.log sh "$HERE/setup.sh" "$SEL"
step gold.log python3 -I "$HERE/gold_lsp.py" "$SEL" "$B3_ROOT/corpora-indep" "$R/gold" --base "$B3_ROOT"
step questions.log python3 -I "$HERE/questions.py" "$SEL" "$R/gold" "$B3_ROOT/corpora-indep" "$R/questions.json" --md "$R/questions.generated.md"
step run.log "$B3_ROOT/venv-indep/bin/python" -I "$HERE/run_indep.py" "$SEL" "$R/gold" "$R/questions.json" "$B3_ROOT/corpora-indep" \
  --tools "${INDEP_TOOLS:-dctx,trail,graphify}"
cat "$R/gold.log" "$R/questions.log" "$R/table.md"
[ -s "$B3_RESULTS/table.md" ] || { echo "no table written" >&2; exit 3; }
echo "INDEP-BENCH DONE $B3_RESULTS"
