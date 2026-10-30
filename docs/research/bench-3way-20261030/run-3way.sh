#!/bin/sh
# One command: ./run-3way.sh <path-to-dctx-binary> [bench3.py args, e.g. --corpora flask,docs --phases corpus]
# Writes results/<timestamp>/{aggregate.json,headline.md,*.jsonl,env.json,outputs/}.
set -eu
[ $# -ge 1 ] || { echo "usage: $0 <dctx-binary> [bench3.py args]" >&2; exit 2; }
DCTX=$1; shift
[ -x "$DCTX" ] || { echo "not executable: $DCTX" >&2; exit 2; }
export B3_ROOT=${B3_ROOT:-$(cd "$(dirname "$0")" && pwd)}
export B3_DCTX=$(readlink -f "$DCTX")
export B3_RESULTS="$B3_ROOT/results/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$B3_RESULTS"
cd "$B3_ROOT/scripts"
sh setup_tools.sh >"$B3_RESULTS/setup_tools.log" 2>&1       # network only here (installs); measured runs are netns-isolated
[ -f "$B3_ROOT/corpora-shas.json" ] || python3 setup_corpora.py >"$B3_RESULTS/setup_corpora.log" 2>&1
[ -f "$B3_ROOT/targets/flask.targets.json" ] || python3 select_targets.py ../corpora ../targets flask ripgrep pocketbase trailcode
python3 bench3.py "$@" 2>&1 | tee "$B3_RESULTS/run.log"
python3 aggregate.py "$B3_RESULTS"
echo "RESULTS: $B3_RESULTS"
